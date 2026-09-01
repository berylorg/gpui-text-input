use super::*;

impl RangePrepublicationSession {
    pub(super) fn admit_external_effect(
        &mut self,
        cleanup: RangePrepublicationCleanupToken,
        effects: &mut EffectBuffer,
        prepared: RangeSurfaceCharge,
        successor: RangeSurfaceCharge,
    ) -> Result<bool, RangePrepublicationFailure> {
        let effect_count = effects
            .len()
            .checked_add(1)
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
        let effect_storage = RangeSurfaceCharge {
            bytes: effect_count
                .checked_mul(std::mem::size_of::<RangePrepublicationEffect>())
                .ok_or(RangePrepublicationFailure::Arithmetic)?,
            items: effect_count,
        };
        let current = self
            .current_charge()
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
        let prepared_peak = add_charge(add_charge(current, prepared)?, effect_storage)?;
        let successor_peak = add_charge(add_charge(current, successor)?, effect_storage)?;
        let peak = RangeSurfaceCharge {
            bytes: prepared_peak.bytes.max(successor_peak.bytes),
            items: prepared_peak.items.max(successor_peak.items),
        };
        self.observe_charge(peak);
        if !charge_fits(peak, configured_capacity(self.environment.config())) {
            let _ = self.environment.cleanup().complete(cleanup);
            return Err(RangePrepublicationFailure::TerminalCapacity);
        }
        if !charge_fits(peak, self.available) {
            let _ = self.environment.cleanup().complete(cleanup);
            self.ledger_blocked = true;
            return Ok(false);
        }
        Ok(true)
    }

    pub(super) fn next_id(&mut self) -> Result<u64, RangePrepublicationFailure> {
        let value = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
        Ok(value)
    }

    pub(super) fn reserve_cleanup(
        &mut self,
    ) -> Result<Option<RangePrepublicationCleanupToken>, RangePrepublicationFailure> {
        match self.environment.cleanup().reserve_request(self.generation) {
            Ok(cleanup) => Ok(Some(cleanup)),
            Err(CleanupRegistrationError::Full) => {
                self.ledger_blocked = true;
                Ok(None)
            }
            Err(CleanupRegistrationError::Arithmetic) => {
                Err(RangePrepublicationFailure::Arithmetic)
            }
        }
    }

    pub(super) fn response_coexistence_charge(&self) -> Option<RangeSurfaceCharge> {
        let current = self.current_charge()?;
        self.delivered.as_ref().map_or(Some(current), |response| {
            add_charge(current, response.charge()?).ok()
        })
    }

    pub(super) fn current_charge(&self) -> Option<RangeSurfaceCharge> {
        let text = self.residency.counts();
        let objects = self.object_residency.counts();
        let geometry =
            self.geometry
                .as_ref()
                .map_or(Some(RangeSurfaceCharge::default()), |geometry| {
                    let counts = geometry.counts();
                    let presentation_overlap = geometry
                        .presentation_overlap_bytes(self.object_residency.resident_page_iter())?;
                    nested_owner_charge(
                        RangeSurfaceCharge {
                            bytes: counts.total_bytes().checked_sub(presentation_overlap)?,
                            items: counts.total_items(),
                        },
                        std::mem::size_of::<ExactGeometryOwner>(),
                    )
                })?;
        let text_owner = nested_owner_charge(
            self.residency.owner_storage_charge(),
            std::mem::size_of::<RangeResidency>(),
        )?;
        let object_owner = nested_owner_charge(
            self.object_residency.owner_storage_charge(),
            std::mem::size_of::<crate::ObjectResidency>(),
        )?;
        let text_resident = self.residency.resident_pages().try_fold(
            RangeSurfaceCharge::default(),
            |total, page| {
                add_charge(
                    total,
                    RangeSurfaceCharge {
                        bytes: page.retained_charge().bytes(),
                        items: page.retained_charge().items(),
                    },
                )
                .ok()
            },
        )?;
        let object_resident = self.object_residency.resident_pages().try_fold(
            RangeSurfaceCharge::default(),
            |total, page| {
                add_charge(
                    total,
                    RangeSurfaceCharge {
                        bytes: page.retained_charge().bytes(),
                        items: page.retained_charge().allocated_items().checked_add(1)?,
                    },
                )
                .ok()
            },
        )?;
        let ledger_records = self
            .environment
            .cleanup()
            .active_session_records(self.generation);
        let ledger = multiply_charge(
            RangePrepublicationCleanupLedger::record_charge(),
            ledger_records,
        )?;
        let candidate =
            self.candidate
                .as_ref()
                .map_or(Some(RangeSurfaceCharge::default()), |candidate| {
                    nested_owner_charge(
                        candidate.retained_charge(),
                        std::mem::size_of::<RangePrepublicationCandidate>(),
                    )
                })?;
        let custody = self.custody_storage_charge()?;
        [
            RangeSurfaceCharge {
                bytes: std::mem::size_of::<Self>(),
                items: 1,
            },
            text_owner,
            object_owner,
            text_resident,
            RangeSurfaceCharge {
                bytes: usize::try_from(text.pending_bytes).ok()?,
                items: text.pending_requests,
            },
            object_resident,
            RangeSurfaceCharge {
                bytes: objects.pending_bytes,
                items: objects
                    .pending_requests
                    .checked_add(objects.pending_objects)?,
            },
            geometry,
            ledger,
            custody,
            candidate,
        ]
        .into_iter()
        .try_fold(RangeSurfaceCharge::default(), |total, charge| {
            Some(RangeSurfaceCharge {
                bytes: total.bytes.checked_add(charge.bytes)?,
                items: total.items.checked_add(charge.items)?,
            })
        })
    }

    pub(super) fn observe_high_water(&mut self) {
        if let Some(charge) = self.response_coexistence_charge() {
            self.observe_charge(charge);
        }
    }

    pub(super) fn observe_charge(&mut self, charge: RangeSurfaceCharge) {
        self.high_water.bytes = self.high_water.bytes.max(charge.bytes);
        self.high_water.items = self.high_water.items.max(charge.items);
    }

    pub(super) fn fail(
        &mut self,
        failure: RangePrepublicationFailure,
        _effects: &mut EffectBuffer,
    ) {
        self.environment
            .cleanup()
            .mark_session_ready(self.generation);
        self.release_all_resident_custody();
        self.cancel_waiting();
        self.delivered = None;
        self.candidate = None;
        if let Some(geometry) = self.geometry.as_mut() {
            let _ = geometry.dispose();
        }
        let _ = self.residency.dispose();
        let _ = self.object_residency.dispose();
        self.geometry_job = None;
        self.stage = SessionStage::Failed(failure);
    }

    pub(super) fn fail_without_effects(&mut self, failure: RangePrepublicationFailure) {
        let mut pending = EffectBuffer::new();
        self.fail(failure, &mut pending);
    }

    pub(super) fn cancel_waiting(&mut self) {
        match self.waiting.take() {
            Some(Waiting::RestorationPage { key, .. } | Waiting::GeometryPage { key, .. }) => {
                let _ = self.residency.cancel(key);
            }
            Some(Waiting::RestorationObject { key, .. } | Waiting::GeometryObject { key, .. }) => {
                let _ = self.object_residency.cancel(key);
            }
            Some(Waiting::Validation(_)) | None => {}
        }
    }
}
