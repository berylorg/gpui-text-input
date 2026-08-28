use std::mem::size_of;

use super::*;

#[derive(Clone)]
pub(in crate::range_widget) enum DeferredGeometryResponse {
    IndexPage(RangePage),
    TargetPage(RangePage),
    IndexObject(crate::ObjectPage),
    TargetObject(crate::ObjectPage),
}

impl DeferredGeometryResponse {
    pub(in crate::range_widget) fn incremental_charge(&self) -> crate::RangeSurfaceCharge {
        match self {
            Self::IndexPage(page) | Self::TargetPage(page) => {
                let charge = page.retained_charge();
                crate::RangeSurfaceCharge {
                    bytes: charge.bytes() - size_of::<RangePage>(),
                    items: charge.items() - 1,
                }
            }
            Self::IndexObject(page) | Self::TargetObject(page) => {
                let charge = page.retained_charge();
                crate::RangeSurfaceCharge {
                    bytes: charge.bytes() - size_of::<crate::ObjectPage>(),
                    items: charge.allocated_items(),
                }
            }
        }
    }

    pub(in crate::range_widget) fn page_key(&self) -> Option<PageRequestKey> {
        match self {
            Self::IndexPage(page) | Self::TargetPage(page) => Some(page.key()),
            Self::IndexObject(_) | Self::TargetObject(_) => None,
        }
    }

    pub(in crate::range_widget) fn object_key(&self) -> Option<ObjectRequestKey> {
        match self {
            Self::IndexObject(page) | Self::TargetObject(page) => Some(page.key()),
            Self::IndexPage(_) | Self::TargetPage(_) => None,
        }
    }

    fn remains_dispatched(&self, input: &RangeTextInput) -> bool {
        self.page_key()
            .is_some_and(|key| input.dispatched_pages.contains(&key))
            || self
                .object_key()
                .is_some_and(|key| input.dispatched_object_pages.contains(&key))
    }
}

impl RangeTextInput {
    pub(in crate::range_widget) fn defer_geometry_response(
        &mut self,
        response: DeferredGeometryResponse,
        _cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if self.deferred_geometry_response.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        let admission = self.deferred_geometry_response_admission_charge(&response)?;
        if admission.bytes > self.config.limits.max_surface_bytes
            || admission.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.observe_surface_admission_peak(admission);
        self.deferred_geometry_response = Some(response);
        self.observe_realization_ownership();
        Ok(())
    }

    pub(in crate::range_widget) fn deferred_geometry_response_admission_charge(
        &self,
        response: &DeferredGeometryResponse,
    ) -> Result<crate::RangeSurfaceCharge, RangeTextInputError> {
        let response_charge = response.incremental_charge();
        let processing_charge = match response {
            DeferredGeometryResponse::IndexPage(page)
            | DeferredGeometryResponse::TargetPage(page) => {
                let charge = page.retained_charge();
                crate::RangeSurfaceCharge {
                    bytes: charge.bytes(),
                    items: charge.items(),
                }
            }
            DeferredGeometryResponse::IndexObject(page)
            | DeferredGeometryResponse::TargetObject(page) => {
                let charge = page.retained_charge();
                crate::RangeSurfaceCharge {
                    bytes: charge.bytes(),
                    items: charge
                        .allocated_items()
                        .checked_add(1)
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                }
            }
        };
        let processing_charge = crate::RangeSurfaceCharge {
            bytes: processing_charge
                .bytes
                .checked_add(size_of::<DeferredGeometryResponse>())
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: processing_charge
                .items
                .checked_add(1)
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        let resident_charge = self.non_surface_resident_charge()?;
        let prior = self
            .surface
            .as_ref()
            .map_or(crate::RangeSurfaceCharge::default(), |surface| {
                surface.charge()
            });
        let request_bytes = self
            .requests
            .capacity()
            .checked_mul(size_of::<RangeTextInputRequest>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let request_payload = super::super::transition::queued_request_payload_charge(
            self.requests.iter(),
            self.clipboard.current_provenance_page(),
        )?;
        let response_custody = self.response_custody_storage_charge();
        let owner = Self::realization_owner_charge();
        let aliases = Self::page_alias_storage_charge(&self.pending_page_aliases)?;
        let residency_owners = [
            self.residency.owner_storage_charge(),
            self.object_residency.owner_storage_charge(),
        ]
        .into_iter()
        .try_fold(crate::RangeSurfaceCharge::default(), |total, charge| {
            Some(crate::RangeSurfaceCharge {
                bytes: total.bytes.checked_add(charge.bytes)?,
                items: total.items.checked_add(charge.items)?,
            })
        })
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let geometry = self.geometry.counts();
        let geometry_bytes = geometry
            .total_bytes()
            .checked_sub(self.geometry_presentation_overlap_bytes()?)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let residency = self.residency.counts();
        let objects = self.object_residency.counts();
        let pending_page_bytes = usize::try_from(residency.pending_bytes)
            .map_err(|_| RangeTextInputError::SurfaceCapacity)?;
        let dispatched_record_bytes = [
            self.dispatched_pages.allocation_charge().bytes,
            self.dispatched_object_pages.allocation_charge().bytes,
            self.dispatched_mutations.allocation_charge().bytes,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let dispatched_record_items = [
            self.dispatched_pages.allocation_charge().items,
            self.dispatched_object_pages.allocation_charge().items,
            self.dispatched_mutations.allocation_charge().items,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        Ok(crate::RangeSurfaceCharge {
            bytes: owner
                .bytes
                .checked_add(prior.bytes)
                .and_then(|total| total.checked_add(request_bytes))
                .and_then(|total| total.checked_add(request_payload.bytes))
                .and_then(|total| total.checked_add(geometry_bytes))
                .and_then(|total| total.checked_add(resident_charge.bytes))
                .and_then(|total| total.checked_add(pending_page_bytes))
                .and_then(|total| total.checked_add(objects.pending_bytes))
                .and_then(|total| total.checked_add(dispatched_record_bytes))
                .and_then(|total| total.checked_add(aliases.bytes))
                .and_then(|total| total.checked_add(residency_owners.bytes))
                .and_then(|total| total.checked_add(response_custody.bytes))
                .and_then(|total| total.checked_add(self.active_response_processing.bytes))
                .and_then(|total| total.checked_add(response_charge.bytes))
                .and_then(|total| total.checked_add(processing_charge.bytes))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: owner
                .items
                .checked_add(prior.items)
                .and_then(|total| total.checked_add(self.requests.capacity()))
                .and_then(|total| total.checked_add(request_payload.items))
                .and_then(|total| total.checked_add(geometry.total_items()))
                .and_then(|total| total.checked_add(resident_charge.items))
                .and_then(|total| total.checked_add(residency.pending_requests))
                .and_then(|total| total.checked_add(objects.pending_requests))
                .and_then(|total| total.checked_add(dispatched_record_items))
                .and_then(|total| total.checked_add(aliases.items))
                .and_then(|total| total.checked_add(residency_owners.items))
                .and_then(|total| total.checked_add(response_custody.items))
                .and_then(|total| total.checked_add(self.active_response_processing.items))
                .and_then(|total| total.checked_add(response_charge.items))
                .and_then(|total| total.checked_add(processing_charge.items))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        })
    }

    pub(in crate::range_widget) fn service_deferred_geometry_response(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<bool, RangeTextInputError> {
        let Some(response) = self.deferred_geometry_response.take() else {
            return Ok(false);
        };
        if !self.try_spend_realization_credit(cx) {
            self.deferred_geometry_response = Some(response);
            return Ok(false);
        }
        let retry = response.clone();
        let result = match response {
            DeferredGeometryResponse::IndexPage(page) => {
                self.deliver_geometry_page_inner(page, true, window, cx)
            }
            DeferredGeometryResponse::TargetPage(page) => {
                self.deliver_geometry_target_page_inner(page, true, window, cx)
            }
            DeferredGeometryResponse::IndexObject(page) => {
                self.deliver_geometry_object_page_inner(page, true, window, cx)
            }
            DeferredGeometryResponse::TargetObject(page) => {
                self.deliver_geometry_target_object_page_inner(page, true, window, cx)
            }
        };
        if let Err(error) = result {
            self.refund_realization_credit();
            if retry.remains_dispatched(self) {
                self.deferred_geometry_response = Some(retry);
                self.schedule_realization_continuation(cx);
                self.observe_realization_ownership();
            }
            return Err(error);
        }
        self.observe_realization_ownership();
        Ok(true)
    }
}
