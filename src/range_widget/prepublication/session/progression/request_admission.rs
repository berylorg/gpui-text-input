use super::super::*;

impl RangePrepublicationSession {
    pub(super) fn admit_resident_geometry_request(
        &mut self,
        request_bytes: usize,
        pending_exists: bool,
        prepared: RangeSurfaceCharge,
        effects: &EffectBuffer,
    ) -> Result<Option<RangeSurfaceCharge>, RangePrepublicationFailure> {
        let pending = if pending_exists {
            RangeSurfaceCharge::default()
        } else {
            RangeSurfaceCharge {
                bytes: request_bytes,
                items: 1,
            }
        };
        let current = self
            .current_charge()
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
        let effect_storage = multiply_charge(
            RangeSurfaceCharge {
                bytes: std::mem::size_of::<RangePrepublicationEffect>(),
                items: 1,
            },
            effects.len(),
        )
        .ok_or(RangePrepublicationFailure::Arithmetic)?;
        let peak = add_charge(
            add_charge(add_charge(current, prepared)?, pending)?,
            effect_storage,
        )?;
        self.observe_charge(peak);
        if !charge_fits(peak, configured_capacity(self.environment.config())) {
            return Err(RangePrepublicationFailure::TerminalCapacity);
        }
        if !charge_fits(peak, self.available) {
            self.ledger_blocked = true;
            return Ok(None);
        }
        let counts = self
            .geometry
            .as_ref()
            .ok_or(RangePrepublicationFailure::Stale)?
            .counts();
        add_charge(
            RangeSurfaceCharge {
                bytes: counts.total_bytes(),
                items: counts.total_items(),
            },
            pending,
        )
        .map(Some)
    }

    pub(super) fn admit_restoration_page_request(
        &mut self,
        prepared: crate::residency::PreparedPageDemand,
        request: crate::PageRequest,
        effects: &mut EffectBuffer,
    ) -> Result<bool, RangePrepublicationFailure> {
        let Some(cleanup) = self.reserve_cleanup()? else {
            return Ok(false);
        };
        let prepared_charge = RangeSurfaceCharge {
            bytes: prepared.retained_bytes(),
            items: prepared.retained_items(),
        };
        let successor_charge = RangeSurfaceCharge {
            bytes: usize::try_from(request.key().demand().max_payload_bytes())
                .map_err(|_| RangePrepublicationFailure::Arithmetic)?,
            items: 1,
        };
        if !self.admit_external_effect(cleanup, effects, prepared_charge, successor_charge)? {
            return Ok(false);
        }
        if !self.environment.cleanup().bind_request(
            cleanup,
            CleanupRequest::Page {
                generation: self.generation,
                key: request.key(),
            },
        ) {
            return Err(RangePrepublicationFailure::Stale);
        }
        if self.residency.commit_prepared_demand(prepared) != PageDemand::Requested(request) {
            return Err(RangePrepublicationFailure::Stale);
        }
        self.validation.begin_text(request.key());
        self.waiting = Some(Waiting::RestorationPage {
            key: request.key(),
            cleanup,
        });
        effects.push(RangePrepublicationEffect::Page {
            cleanup,
            generation: self.generation,
            request,
        });
        Ok(true)
    }

    pub(super) fn admit_restoration_object_request(
        &mut self,
        prepared: crate::object_residency::PreparedObjectDemand,
        request: crate::ObjectRequest,
        effects: &mut EffectBuffer,
    ) -> Result<bool, RangePrepublicationFailure> {
        let Some(cleanup) = self.reserve_cleanup()? else {
            return Ok(false);
        };
        let reservation = object_reservation(request.key())?;
        let prepared_charge = add_charge(
            RangeSurfaceCharge {
                bytes: prepared.retained_bytes(),
                items: prepared.retained_items(),
            },
            reservation,
        )?;
        if !self.admit_external_effect(cleanup, effects, prepared_charge, reservation)? {
            return Ok(false);
        }
        if !self.environment.cleanup().bind_request(
            cleanup,
            CleanupRequest::ObjectPage {
                generation: self.generation,
                key: request.key(),
            },
        ) {
            return Err(RangePrepublicationFailure::Stale);
        }
        if self.object_residency.commit_prepared_demand(prepared)
            != ObjectDemand::Requested(request)
        {
            return Err(RangePrepublicationFailure::Stale);
        }
        self.validation.begin_object(request.key());
        self.waiting = Some(Waiting::RestorationObject {
            key: request.key(),
            cleanup,
        });
        effects.push(RangePrepublicationEffect::ObjectPage {
            cleanup,
            generation: self.generation,
            request,
        });
        Ok(true)
    }

    pub(super) fn admit_geometry_object_request(
        &mut self,
        job: GeometryJobKey,
        text_page: PageId,
        request_id: ObjectRequestId,
        request: crate::ObjectRequest,
        pending_exists: bool,
        prepared: crate::object_residency::PreparedObjectDemand,
        resident_request: crate::ObjectRequest,
        effects: &mut EffectBuffer,
    ) -> Result<bool, RangePrepublicationFailure> {
        let Some(cleanup) = self.reserve_cleanup()? else {
            return Ok(false);
        };
        let reservation = object_reservation(request.key())?;
        let geometry_pending = if pending_exists {
            RangeSurfaceCharge::default()
        } else {
            RangeSurfaceCharge {
                bytes: std::mem::size_of::<crate::ObjectRequestKey>(),
                items: 1,
            }
        };
        let prepared_charge = add_charge(
            add_charge(
                RangeSurfaceCharge {
                    bytes: prepared.retained_bytes(),
                    items: prepared.retained_items(),
                },
                reservation,
            )?,
            geometry_pending,
        )?;
        let successor_charge = add_charge(reservation, geometry_pending)?;
        if !self.admit_external_effect(cleanup, effects, prepared_charge, successor_charge)? {
            return Ok(false);
        }
        if !self.environment.cleanup().bind_request(
            cleanup,
            CleanupRequest::ObjectPage {
                generation: self.generation,
                key: request.key(),
            },
        ) {
            return Err(RangePrepublicationFailure::Stale);
        }
        let committed = if pending_exists {
            request
        } else {
            self.geometry
                .as_mut()
                .ok_or(RangePrepublicationFailure::Stale)?
                .request_object_page(
                    job,
                    request_id,
                    self.environment
                        .config()
                        .object_residency_limits
                        .max_resident_objects(),
                    self.environment
                        .config()
                        .object_residency_limits
                        .max_resident_bytes(),
                )
                .map_err(classify_geometry_error)?
        };
        if committed != request
            || self.object_residency.commit_prepared_demand(prepared)
                != ObjectDemand::Requested(resident_request)
        {
            return Err(RangePrepublicationFailure::Stale);
        }
        self.waiting = Some(Waiting::GeometryObject {
            job,
            key: request.key(),
            text_page,
            cleanup,
        });
        effects.push(RangePrepublicationEffect::ObjectPage {
            cleanup,
            generation: self.generation,
            request,
        });
        Ok(true)
    }

    pub(super) fn admit_geometry_page_request(
        &mut self,
        job: GeometryJobKey,
        request_id: PageRequestId,
        request: crate::PageRequest,
        pending_exists: bool,
        prepared: crate::residency::PreparedPageDemand,
        resident_request: crate::PageRequest,
        effects: &mut EffectBuffer,
    ) -> Result<bool, RangePrepublicationFailure> {
        let Some(cleanup) = self.reserve_cleanup()? else {
            return Ok(false);
        };
        let reservation = RangeSurfaceCharge {
            bytes: usize::try_from(request.key().demand().max_payload_bytes())
                .map_err(|_| RangePrepublicationFailure::Arithmetic)?,
            items: 1,
        };
        let geometry_pending = if pending_exists {
            RangeSurfaceCharge::default()
        } else {
            RangeSurfaceCharge {
                bytes: std::mem::size_of::<crate::PageRequestKey>(),
                items: 1,
            }
        };
        let prepared_charge = add_charge(
            RangeSurfaceCharge {
                bytes: prepared.retained_bytes(),
                items: prepared.retained_items(),
            },
            geometry_pending,
        )?;
        let successor_charge = add_charge(reservation, geometry_pending)?;
        if !self.admit_external_effect(cleanup, effects, prepared_charge, successor_charge)? {
            return Ok(false);
        }
        if !self.environment.cleanup().bind_request(
            cleanup,
            CleanupRequest::Page {
                generation: self.generation,
                key: request.key(),
            },
        ) {
            return Err(RangePrepublicationFailure::Stale);
        }
        let committed = if pending_exists {
            request
        } else {
            self.geometry
                .as_mut()
                .ok_or(RangePrepublicationFailure::Stale)?
                .request_page(job, request_id)
                .map_err(classify_geometry_error)?
        };
        if committed != request
            || self.residency.commit_prepared_demand(prepared)
                != PageDemand::Requested(resident_request)
        {
            return Err(RangePrepublicationFailure::Stale);
        }
        self.waiting = Some(Waiting::GeometryPage {
            job,
            key: request.key(),
            cleanup,
        });
        effects.push(RangePrepublicationEffect::Page {
            cleanup,
            generation: self.generation,
            request,
        });
        Ok(true)
    }
}

fn object_reservation(
    key: crate::ObjectRequestKey,
) -> Result<RangeSurfaceCharge, RangePrepublicationFailure> {
    Ok(RangeSurfaceCharge {
        bytes: key.demand().max_retained_bytes(),
        items: key
            .demand()
            .max_objects()
            .checked_add(1)
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
    })
}
