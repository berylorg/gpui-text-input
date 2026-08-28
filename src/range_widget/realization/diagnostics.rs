use super::super::*;

impl RangeTextInput {
    pub fn realization_diagnostics(&self) -> RangeRealizationDiagnostics {
        let surface = self.surface.as_ref();
        RangeRealizationDiagnostics {
            max_surface_bytes: self.config.limits.max_surface_bytes,
            max_surface_items: self.config.limits.max_surface_items,
            max_realization_work_per_frame: self.config.limits.max_realization_work_per_frame,
            max_realized_block_extent: self.config.limits.max_realized_block_extent,
            max_resident_pages: self.config.residency_limits.max_resident_pages(),
            max_resident_page_bytes: self.config.residency_limits.max_resident_bytes(),
            max_owned_pages: self.config.residency_limits.max_resident_pages() * 2,
            max_pending_page_requests: self.config.residency_limits.max_pending_requests(),
            max_pending_page_bytes: self.config.residency_limits.max_pending_bytes(),
            max_resident_object_pages: self.config.object_residency_limits.max_resident_pages(),
            max_resident_objects: self.config.object_residency_limits.max_resident_objects(),
            max_resident_object_bytes: self.config.object_residency_limits.max_resident_bytes(),
            max_owned_objects: self.config.object_residency_limits.max_resident_objects() * 2,
            max_pending_object_requests: self.config.object_residency_limits.max_pending_requests(),
            max_pending_object_bytes: self.config.object_residency_limits.max_pending_bytes(),
            max_queued_requests: super::super::checked_request_capacity(&self.config)
                .expect("validated request capacity remains representable"),
            max_geometry_bytes: self.config.geometry_limits.max_retained_bytes(),
            max_geometry_items: self.config.geometry_limits.max_retained_items(),
            max_checkpoints: self.config.geometry_limits.max_checkpoints(),
            frame_generation: self.realization_frame_generation,
            continuation_scheduled: self.realization_continuation_scheduled,
            frame: self.last_realization_step,
            capacity: surface.map_or(RangeRealizationCapacityState::Normal, |surface| {
                surface.capacity_state()
            }),
            filler_count: surface.map_or(0, CoherentRangeSurface::filler_count),
            current: self.current_realization_ownership(),
            high_water: self.realization_high_water,
            surface_charge: surface
                .map_or(RangeSurfaceCharge::default(), |surface| surface.charge()),
            surface_high_water: self.surface_high_water,
            geometry_high_water_bytes: self.geometry.retained_high_water_bytes(),
            geometry_high_water_items: self.geometry.retained_high_water_items(),
            last_response_rejection: self.last_response_rejection,
            response_rejection_count: self.response_rejection_count,
            last_response_rejection_stage: self.last_response_rejection_stage,
        }
    }

    pub(in crate::range_widget) fn current_realization_ownership(
        &self,
    ) -> RangeRealizationOwnership {
        let residency = self.residency.counts();
        let objects = self.object_residency.counts();
        let surface_pages = self
            .surface
            .as_ref()
            .map_or(&[][..], |surface| surface.pages());
        let surface_object_pages = self
            .surface
            .as_ref()
            .map_or(&[][..], |surface| surface.object_pages());
        let resident_pages = surface_pages
            .len()
            .checked_add(self.residency.resident_pages().count())
            .expect("validated page ownership fits usize");
        let surface_objects = surface_object_pages
            .iter()
            .try_fold(0usize, |total, page| {
                total.checked_add(page.objects().len())
            })
            .expect("validated object ownership fits usize");
        let resident_objects = surface_objects
            .checked_add(
                self.object_residency
                    .resident_pages()
                    .try_fold(0usize, |total, page| {
                        total.checked_add(page.objects().len())
                    })
                    .expect("validated object ownership fits usize"),
            )
            .expect("validated object ownership fits usize");
        let resident_page_bytes = surface_pages
            .iter()
            .map(|page| page.retained_charge().bytes())
            .chain(
                self.residency
                    .resident_pages()
                    .map(|page| page.retained_charge().bytes()),
            )
            .try_fold(0usize, usize::checked_add)
            .expect("validated page bytes fit usize");
        let resident_object_bytes = surface_object_pages
            .iter()
            .map(|page| page.retained_charge().bytes())
            .chain(
                self.object_residency
                    .resident_pages()
                    .map(|page| page.retained_charge().bytes()),
            )
            .try_fold(0usize, usize::checked_add)
            .expect("validated object bytes fit usize");
        let non_surface_page_charge = self
            .residency
            .resident_pages()
            .try_fold(RangeSurfaceCharge::default(), |charge, page| {
                let page = page.retained_charge();
                Some(RangeSurfaceCharge {
                    bytes: charge.bytes.checked_add(page.bytes())?,
                    items: charge.items.checked_add(page.items())?,
                })
            })
            .expect("validated page charge fits usize");
        let non_surface_object_charge = self
            .object_residency
            .resident_pages()
            .try_fold(RangeSurfaceCharge::default(), |charge, page| {
                Some(RangeSurfaceCharge {
                    bytes: charge.bytes.checked_add(page.retained_charge().bytes())?,
                    items: charge
                        .items
                        .checked_add(page.objects().len())?
                        .checked_add(1)?,
                })
            })
            .expect("validated object charge fits usize");
        let surface_charge = self
            .surface
            .as_ref()
            .map_or(RangeSurfaceCharge::default(), CoherentRangeSurface::charge);
        let geometry = self.geometry.counts();
        let request_storage = RangeSurfaceCharge {
            bytes: self
                .requests
                .capacity()
                .checked_mul(std::mem::size_of::<RangeTextInputRequest>())
                .expect("validated request storage fits usize"),
            items: self.requests.capacity(),
        };
        let request_payload =
            super::super::transition::queued_request_payload_charge(self.requests.iter())
                .expect("admitted request payload fits usize");
        let deferred = self
            .deferred_geometry_response
            .as_ref()
            .map_or(RangeSurfaceCharge::default(), |response| {
                response.incremental_charge()
            });
        let response_custody = self.response_custody_storage_charge();
        let aliases = Self::page_alias_storage_charge(&self.pending_page_aliases)
            .expect("admitted page alias storage fits usize");
        let pending_configuration = self.pending_layout_intent.as_ref().map_or(
            RangeSurfaceCharge::default(),
            super::PendingLayoutIntent::charge,
        );
        let pending_rebind = self.pending_rebind_intent.as_ref().map_or(
            RangeSurfaceCharge::default(),
            super::PendingRebindIntent::charge,
        );
        let residency_owners = [
            self.residency.owner_storage_charge(),
            self.object_residency.owner_storage_charge(),
        ]
        .into_iter()
        .try_fold(RangeSurfaceCharge::default(), |total, charge| {
            Some(RangeSurfaceCharge {
                bytes: total.bytes.checked_add(charge.bytes)?,
                items: total.items.checked_add(charge.items)?,
            })
        })
        .expect("validated residency owner storage fits usize");
        let candidate_record = RangeSurfaceCharge {
            bytes: usize::from(self.surface_candidate.is_some())
                * std::mem::size_of::<SurfaceCandidate>(),
            items: usize::from(self.surface_candidate.is_some()),
        };
        let pending_geometry_records = RangeSurfaceCharge {
            bytes: usize::from(self.pending_geometry_page.is_some())
                * std::mem::size_of::<geometry::PendingGeometryPage>()
                + usize::from(self.pending_geometry_object.is_some())
                    * std::mem::size_of::<geometry::PendingGeometryObject>(),
            items: usize::from(self.pending_geometry_page.is_some())
                + usize::from(self.pending_geometry_object.is_some()),
        };
        let dispatched_records = [
            self.dispatched_pages.allocation_charge(),
            self.dispatched_object_pages.allocation_charge(),
            self.dispatched_mutations.allocation_charge(),
        ]
        .into_iter()
        .try_fold(RangeSurfaceCharge::default(), |total, charge| {
            Some(RangeSurfaceCharge {
                bytes: total.bytes.checked_add(charge.bytes)?,
                items: total.items.checked_add(charge.items)?,
            })
        })
        .expect("validated dispatched storage fits usize");
        let pending_page_bytes = usize::try_from(residency.pending_bytes)
            .expect("validated pending page bytes fit usize");
        let owned_bytes = [
            Self::realization_owner_charge().bytes,
            surface_charge.bytes,
            non_surface_page_charge.bytes,
            non_surface_object_charge.bytes,
            pending_page_bytes,
            objects.pending_bytes,
            geometry.total_bytes(),
            request_storage.bytes,
            request_payload.bytes,
            deferred.bytes,
            response_custody.bytes,
            self.active_response_processing.bytes,
            aliases.bytes,
            pending_configuration.bytes,
            pending_rebind.bytes,
            dispatched_records.bytes,
            residency_owners.bytes,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .expect("admitted realization bytes fit usize");
        let owned_items = [
            Self::realization_owner_charge().items,
            surface_charge.items,
            non_surface_page_charge.items,
            non_surface_object_charge.items,
            residency.pending_requests,
            objects.pending_requests,
            geometry.total_items(),
            request_storage.items,
            request_payload.items,
            deferred.items,
            response_custody.items,
            self.active_response_processing.items,
            aliases.items,
            pending_configuration.items,
            pending_rebind.items,
            dispatched_records.items,
            residency_owners.items,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .expect("admitted realization items fit usize");
        RangeRealizationOwnership {
            owned_bytes,
            owned_items,
            resident_page_bytes,
            resident_object_bytes,
            pending_page_bytes,
            pending_object_bytes: objects.pending_bytes,
            geometry_bytes: geometry.total_bytes(),
            geometry_items: geometry.total_items(),
            request_storage_bytes: request_storage.bytes,
            request_storage_items: request_storage.items,
            request_payload_bytes: request_payload.bytes,
            request_payload_items: request_payload.items,
            deferred_response_bytes: deferred.bytes,
            deferred_response_items: deferred.items,
            response_custody_bytes: response_custody.bytes,
            response_custody_items: response_custody.items,
            response_custody_count: self.response_custody.len(),
            response_processing_bytes: self.active_response_processing.bytes,
            response_processing_items: self.active_response_processing.items,
            page_alias_storage_bytes: aliases.bytes,
            page_alias_storage_items: aliases.items,
            page_alias_waits: self.pending_page_aliases.len(),
            pending_configuration_bytes: pending_configuration
                .bytes
                .checked_add(pending_rebind.bytes)
                .expect("admitted pending configuration bytes fit usize"),
            pending_configuration_items: pending_configuration
                .items
                .checked_add(pending_rebind.items)
                .expect("admitted pending configuration items fit usize"),
            candidate_bytes: candidate_record.bytes,
            candidate_items: candidate_record.items,
            pending_geometry_record_bytes: pending_geometry_records.bytes,
            pending_geometry_record_items: pending_geometry_records.items,
            dispatched_record_bytes: dispatched_records.bytes,
            dispatched_record_items: dispatched_records.items,
            resident_pages,
            resident_objects,
            pending_page_requests: residency.pending_requests,
            pending_object_requests: objects.pending_requests,
            dispatched_page_requests: self.dispatched_pages.len(),
            dispatched_object_requests: self.dispatched_object_pages.len(),
            active_geometry_jobs: usize::from(self.active_geometry.is_some()),
            pending_geometry_pages: usize::from(self.pending_geometry_page.is_some()),
            pending_geometry_objects: usize::from(self.pending_geometry_object.is_some()),
            resident_geometry_page_waits: usize::from(
                self.pending_geometry_page.as_ref().is_some_and(|pending| {
                    matches!(pending.wait, geometry::GeometryPageWait::Resident(_))
                }),
            ),
            coalesced_geometry_page_waits: usize::from(
                self.pending_geometry_page.as_ref().is_some_and(|pending| {
                    matches!(pending.wait, geometry::GeometryPageWait::Coalesced(_))
                }),
            ),
            index_geometry_page_waits: usize::from(
                self.pending_geometry_page.as_ref().is_some_and(|pending| {
                    pending.request.key().purpose() == crate::PagePurpose::GeometryIndex
                }),
            ),
            target_geometry_page_waits: usize::from(
                self.pending_geometry_page.as_ref().is_some_and(|pending| {
                    pending.request.key().purpose() == crate::PagePurpose::GeometryTarget
                }),
            ),
            deferred_geometry_responses: usize::from(self.deferred_geometry_response.is_some()),
            pending_target_intents: usize::from(self.pending_target_intent.is_some()),
            pending_index_intents: usize::from(self.pending_index_intent),
            pending_layout_intents: usize::from(self.pending_layout_intent.is_some()),
            pending_presentation_intents: usize::from(self.pending_presentation_intent.is_some()),
            pending_rebind_intents: usize::from(self.pending_rebind_intent.is_some()),
            scheduled_continuations: usize::from(self.realization_continuation_scheduled),
            queued_requests: self.requests.len(),
            candidates: usize::from(self.surface_candidate.is_some()),
            checkpoints: self
                .geometry
                .index()
                .map_or(0, |index| index.checkpoints().len()),
        }
    }

    pub(in crate::range_widget) fn observe_realization_ownership(&mut self) {
        let current = self.current_realization_ownership();
        self.realization_high_water.owned_bytes = self
            .realization_high_water
            .owned_bytes
            .max(current.owned_bytes);
        self.realization_high_water.owned_items = self
            .realization_high_water
            .owned_items
            .max(current.owned_items);
        self.realization_high_water.resident_page_bytes = self
            .realization_high_water
            .resident_page_bytes
            .max(current.resident_page_bytes);
        self.realization_high_water.resident_object_bytes = self
            .realization_high_water
            .resident_object_bytes
            .max(current.resident_object_bytes);
        self.realization_high_water.pending_page_bytes = self
            .realization_high_water
            .pending_page_bytes
            .max(current.pending_page_bytes);
        self.realization_high_water.pending_object_bytes = self
            .realization_high_water
            .pending_object_bytes
            .max(current.pending_object_bytes);
        self.realization_high_water.geometry_bytes = self
            .realization_high_water
            .geometry_bytes
            .max(current.geometry_bytes);
        self.realization_high_water.geometry_items = self
            .realization_high_water
            .geometry_items
            .max(current.geometry_items);
        self.realization_high_water.request_storage_bytes = self
            .realization_high_water
            .request_storage_bytes
            .max(current.request_storage_bytes);
        self.realization_high_water.request_storage_items = self
            .realization_high_water
            .request_storage_items
            .max(current.request_storage_items);
        self.realization_high_water.request_payload_bytes = self
            .realization_high_water
            .request_payload_bytes
            .max(current.request_payload_bytes);
        self.realization_high_water.request_payload_items = self
            .realization_high_water
            .request_payload_items
            .max(current.request_payload_items);
        self.realization_high_water.deferred_response_bytes = self
            .realization_high_water
            .deferred_response_bytes
            .max(current.deferred_response_bytes);
        self.realization_high_water.deferred_response_items = self
            .realization_high_water
            .deferred_response_items
            .max(current.deferred_response_items);
        self.realization_high_water.response_custody_bytes = self
            .realization_high_water
            .response_custody_bytes
            .max(current.response_custody_bytes);
        self.realization_high_water.response_custody_items = self
            .realization_high_water
            .response_custody_items
            .max(current.response_custody_items);
        self.realization_high_water.response_custody_count = self
            .realization_high_water
            .response_custody_count
            .max(current.response_custody_count);
        self.realization_high_water.response_processing_bytes = self
            .realization_high_water
            .response_processing_bytes
            .max(current.response_processing_bytes);
        self.realization_high_water.response_processing_items = self
            .realization_high_water
            .response_processing_items
            .max(current.response_processing_items);
        self.realization_high_water.page_alias_storage_bytes = self
            .realization_high_water
            .page_alias_storage_bytes
            .max(current.page_alias_storage_bytes);
        self.realization_high_water.page_alias_storage_items = self
            .realization_high_water
            .page_alias_storage_items
            .max(current.page_alias_storage_items);
        self.realization_high_water.page_alias_waits = self
            .realization_high_water
            .page_alias_waits
            .max(current.page_alias_waits);
        self.realization_high_water.pending_configuration_bytes = self
            .realization_high_water
            .pending_configuration_bytes
            .max(current.pending_configuration_bytes);
        self.realization_high_water.pending_configuration_items = self
            .realization_high_water
            .pending_configuration_items
            .max(current.pending_configuration_items);
        self.realization_high_water.candidate_bytes = self
            .realization_high_water
            .candidate_bytes
            .max(current.candidate_bytes);
        self.realization_high_water.candidate_items = self
            .realization_high_water
            .candidate_items
            .max(current.candidate_items);
        self.realization_high_water.pending_geometry_record_bytes = self
            .realization_high_water
            .pending_geometry_record_bytes
            .max(current.pending_geometry_record_bytes);
        self.realization_high_water.pending_geometry_record_items = self
            .realization_high_water
            .pending_geometry_record_items
            .max(current.pending_geometry_record_items);
        self.realization_high_water.dispatched_record_bytes = self
            .realization_high_water
            .dispatched_record_bytes
            .max(current.dispatched_record_bytes);
        self.realization_high_water.dispatched_record_items = self
            .realization_high_water
            .dispatched_record_items
            .max(current.dispatched_record_items);
        self.realization_high_water.resident_pages = self
            .realization_high_water
            .resident_pages
            .max(current.resident_pages);
        self.realization_high_water.resident_objects = self
            .realization_high_water
            .resident_objects
            .max(current.resident_objects);
        self.realization_high_water.pending_page_requests = self
            .realization_high_water
            .pending_page_requests
            .max(current.pending_page_requests);
        self.realization_high_water.pending_object_requests = self
            .realization_high_water
            .pending_object_requests
            .max(current.pending_object_requests);
        self.realization_high_water.dispatched_page_requests = self
            .realization_high_water
            .dispatched_page_requests
            .max(current.dispatched_page_requests);
        self.realization_high_water.dispatched_object_requests = self
            .realization_high_water
            .dispatched_object_requests
            .max(current.dispatched_object_requests);
        self.realization_high_water.active_geometry_jobs = self
            .realization_high_water
            .active_geometry_jobs
            .max(current.active_geometry_jobs);
        self.realization_high_water.pending_geometry_pages = self
            .realization_high_water
            .pending_geometry_pages
            .max(current.pending_geometry_pages);
        self.realization_high_water.pending_geometry_objects = self
            .realization_high_water
            .pending_geometry_objects
            .max(current.pending_geometry_objects);
        self.realization_high_water.resident_geometry_page_waits = self
            .realization_high_water
            .resident_geometry_page_waits
            .max(current.resident_geometry_page_waits);
        self.realization_high_water.coalesced_geometry_page_waits = self
            .realization_high_water
            .coalesced_geometry_page_waits
            .max(current.coalesced_geometry_page_waits);
        self.realization_high_water.index_geometry_page_waits = self
            .realization_high_water
            .index_geometry_page_waits
            .max(current.index_geometry_page_waits);
        self.realization_high_water.target_geometry_page_waits = self
            .realization_high_water
            .target_geometry_page_waits
            .max(current.target_geometry_page_waits);
        self.realization_high_water.deferred_geometry_responses = self
            .realization_high_water
            .deferred_geometry_responses
            .max(current.deferred_geometry_responses);
        self.realization_high_water.pending_target_intents = self
            .realization_high_water
            .pending_target_intents
            .max(current.pending_target_intents);
        self.realization_high_water.pending_index_intents = self
            .realization_high_water
            .pending_index_intents
            .max(current.pending_index_intents);
        self.realization_high_water.pending_layout_intents = self
            .realization_high_water
            .pending_layout_intents
            .max(current.pending_layout_intents);
        self.realization_high_water.pending_presentation_intents = self
            .realization_high_water
            .pending_presentation_intents
            .max(current.pending_presentation_intents);
        self.realization_high_water.pending_rebind_intents = self
            .realization_high_water
            .pending_rebind_intents
            .max(current.pending_rebind_intents);
        self.realization_high_water.scheduled_continuations = self
            .realization_high_water
            .scheduled_continuations
            .max(current.scheduled_continuations);
        self.realization_high_water.queued_requests = self
            .realization_high_water
            .queued_requests
            .max(current.queued_requests);
        self.realization_high_water.candidates = self
            .realization_high_water
            .candidates
            .max(current.candidates);
        self.realization_high_water.checkpoints = self
            .realization_high_water
            .checkpoints
            .max(current.checkpoints);
        if let Some(surface) = &self.surface {
            let charge = surface.charge();
            self.surface_high_water.bytes = self.surface_high_water.bytes.max(charge.bytes);
            self.surface_high_water.items = self.surface_high_water.items.max(charge.items);
        }
    }

    pub(in crate::range_widget) fn observe_realization_peak(
        &mut self,
        peak: RangeSurfaceCharge,
        alias_candidate: Option<(RangeSurfaceCharge, usize)>,
    ) {
        self.realization_high_water.owned_bytes =
            self.realization_high_water.owned_bytes.max(peak.bytes);
        self.realization_high_water.owned_items =
            self.realization_high_water.owned_items.max(peak.items);
        if let Some((charge, waits)) = alias_candidate {
            self.realization_high_water.page_alias_storage_bytes = self
                .realization_high_water
                .page_alias_storage_bytes
                .max(charge.bytes);
            self.realization_high_water.page_alias_storage_items = self
                .realization_high_water
                .page_alias_storage_items
                .max(charge.items);
            self.realization_high_water.page_alias_waits =
                self.realization_high_water.page_alias_waits.max(waits);
        }
    }

    pub(in crate::range_widget) fn observe_surface_admission_peak(
        &mut self,
        admission: RangeSurfaceCharge,
    ) {
        self.realization_high_water.owned_bytes =
            self.realization_high_water.owned_bytes.max(admission.bytes);
        self.realization_high_water.owned_items =
            self.realization_high_water.owned_items.max(admission.items);
        self.surface_high_water.bytes = self.surface_high_water.bytes.max(admission.bytes);
        self.surface_high_water.items = self.surface_high_water.items.max(admission.items);
    }
}
