use super::*;

#[derive(Clone, Debug)]
pub(super) enum RangeResponseCustody {
    Page(crate::RangePage),
    PageNoAliases(crate::RangePage),
    ResidentPage(crate::RangePage),
    AliasFanout(AliasFanout),
    Object(crate::ObjectPage),
}

#[derive(Clone, Debug)]
pub(super) struct AliasFanout {
    pub(super) page: crate::RangePage,
    pub(super) cursor: usize,
    pub(super) matched: bool,
}

impl RangeResponseCustody {
    pub(super) fn page(&self) -> Option<&crate::RangePage> {
        match self {
            Self::Page(page) | Self::PageNoAliases(page) | Self::ResidentPage(page) => Some(page),
            Self::AliasFanout(fanout) => Some(&fanout.page),
            Self::Object(_) => None,
        }
    }
    fn incremental_charge(&self) -> Result<RangeSurfaceCharge, RangeTextInputError> {
        match self {
            Self::Page(page) | Self::PageNoAliases(page) | Self::ResidentPage(page) => {
                let charge = page.retained_charge();
                Ok(RangeSurfaceCharge {
                    bytes: charge
                        .bytes()
                        .checked_sub(std::mem::size_of::<crate::RangePage>())
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                    items: charge.items().checked_sub(1).unwrap_or(0),
                })
            }
            Self::AliasFanout(fanout) => {
                let charge = fanout.page.retained_charge();
                Ok(RangeSurfaceCharge {
                    bytes: charge
                        .bytes()
                        .checked_sub(std::mem::size_of::<crate::RangePage>())
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                    items: charge.items().checked_sub(1).unwrap_or(0),
                })
            }
            Self::Object(page) => {
                let charge = page.retained_charge();
                Ok(RangeSurfaceCharge {
                    bytes: charge
                        .bytes()
                        .checked_sub(std::mem::size_of::<crate::ObjectPage>())
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                    items: charge.objects(),
                })
            }
        }
    }

    fn processing_charge(&self) -> Result<RangeSurfaceCharge, RangeTextInputError> {
        match self {
            Self::Page(page) | Self::PageNoAliases(page) | Self::ResidentPage(page) => {
                let charge = page.retained_charge();
                Ok(RangeSurfaceCharge {
                    bytes: charge.bytes(),
                    items: charge.items(),
                })
            }
            Self::AliasFanout(fanout) => {
                let charge = fanout.page.retained_charge();
                Ok(RangeSurfaceCharge {
                    bytes: charge.bytes(),
                    items: charge.items(),
                })
            }
            Self::Object(page) => {
                let charge = page.retained_charge();
                Ok(RangeSurfaceCharge {
                    bytes: charge.bytes(),
                    items: charge
                        .objects()
                        .checked_add(1)
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                })
            }
        }
    }

    fn remains_dispatched(&self, input: &RangeTextInput) -> bool {
        match self {
            Self::Page(page) => input.dispatched_pages.contains(&page.key()),
            Self::PageNoAliases(page) => input.dispatched_pages.contains(&page.key()),
            Self::ResidentPage(page) => input.range_continuation_waits_on(page.key()),
            Self::AliasFanout(fanout) => {
                fanout.matched || fanout.cursor < input.pending_page_aliases.len()
            }
            Self::Object(page) => input.dispatched_object_pages.contains(&page.key()),
        }
    }

    fn geometry_alignment_is_current(&self, input: &RangeTextInput) -> bool {
        match self {
            Self::Object(page)
                if matches!(
                    page.key().purpose(),
                    crate::ObjectPurpose::GeometryIndex | crate::ObjectPurpose::GeometryTarget
                ) =>
            {
                let key = page.key();
                let Some(pending) = input.pending_geometry_object.as_ref() else {
                    return false;
                };
                input.dispatched_object_pages.contains(&key)
                    && pending.request.key() == key
                    && input.active_geometry == Some(pending.job)
                    && matches!(pending.wait, geometry::GeometryObjectWait::Coalesced(wait) if wait == key)
                    && (!matches!(key.purpose(), crate::ObjectPurpose::GeometryTarget)
                        || input
                            .surface_candidate
                            .as_ref()
                            .is_some_and(|candidate| candidate.job == pending.job))
            }
            Self::Page(page) | Self::PageNoAliases(page)
                if matches!(
                    page.key().purpose(),
                    crate::PagePurpose::GeometryIndex | crate::PagePurpose::GeometryTarget
                ) =>
            {
                let key = page.key();
                let Some(pending) = input.pending_geometry_page.as_ref() else {
                    return false;
                };
                input.dispatched_pages.contains(&key)
                    && pending.request.key() == key
                    && input.active_geometry == Some(pending.job)
                    && matches!(pending.wait, geometry::GeometryPageWait::Coalesced(wait) if wait == key)
                    && (!matches!(key.purpose(), crate::PagePurpose::GeometryTarget)
                        || input
                            .surface_candidate
                            .as_ref()
                            .is_some_and(|candidate| candidate.job == pending.job))
            }
            _ => true,
        }
    }

    fn transient_charge(&self) -> Result<RangeSurfaceCharge, RangeTextInputError> {
        let incremental = self.incremental_charge()?;
        let processing = self.processing_charge()?;
        Ok(RangeSurfaceCharge {
            bytes: incremental
                .bytes
                .checked_add(processing.bytes)
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: incremental
                .items
                .checked_add(processing.items)
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        })
    }
}

impl RangeTextInput {
    fn record_response_rejection(
        &mut self,
        response: &RangeResponseCustody,
        error: &RangeTextInputError,
    ) {
        use crate::ExactGeometryError;

        self.last_response_rejection_stage =
            self.pending_response_exact_geometry_failure_stage.take();
        self.last_response_rejection = Some(match error {
            RangeTextInputError::Stale if !response.geometry_alignment_is_current(self) => {
                RangeResponseRejectionClass::AlignmentKeyJobStale
            }
            RangeTextInputError::Stale => RangeResponseRejectionClass::ResidencyStale,
            RangeTextInputError::SurfaceCapacity => RangeResponseRejectionClass::ResidencyCapacity,
            RangeTextInputError::Geometry(ExactGeometryError::CapacityExceeded) => {
                RangeResponseRejectionClass::ExactGeometryCapacity
            }
            RangeTextInputError::Geometry(ExactGeometryError::Busy)
            | RangeTextInputError::Busy
            | RangeTextInputError::Pending => RangeResponseRejectionClass::Busy,
            RangeTextInputError::Geometry(
                ExactGeometryError::NoActiveJob | ExactGeometryError::ObsoleteJob(_),
            ) => RangeResponseRejectionClass::ExactGeometryInactiveOrWrongJob,
            RangeTextInputError::Geometry(ExactGeometryError::InvalidLimits) => {
                RangeResponseRejectionClass::ExactGeometryInvalidLimits
            }
            RangeTextInputError::Geometry(ExactGeometryError::InvalidMetric) => {
                RangeResponseRejectionClass::ExactGeometryInvalidMetric
            }
            RangeTextInputError::Geometry(ExactGeometryError::Disposed) => {
                RangeResponseRejectionClass::ExactGeometryDisposed
            }
            RangeTextInputError::Geometry(ExactGeometryError::EpochExhausted) => {
                RangeResponseRejectionClass::ExactGeometryEpochExhausted
            }
            RangeTextInputError::Geometry(ExactGeometryError::IdNotMonotonic) => {
                RangeResponseRejectionClass::ExactGeometryIdNotMonotonic
            }
            RangeTextInputError::Geometry(ExactGeometryError::IndexIncomplete) => {
                RangeResponseRejectionClass::ExactGeometryIndexIncomplete
            }
            RangeTextInputError::Geometry(ExactGeometryError::PageAlreadyPending) => {
                RangeResponseRejectionClass::ExactGeometryPageAlreadyPending
            }
            RangeTextInputError::Geometry(ExactGeometryError::WrongPage(_)) => {
                RangeResponseRejectionClass::ExactGeometryWrongPage
            }
            RangeTextInputError::Geometry(ExactGeometryError::WrongInputKind) => {
                RangeResponseRejectionClass::ExactGeometryWrongInputKind
            }
            RangeTextInputError::Geometry(ExactGeometryError::WrongObjectPage(_)) => {
                RangeResponseRejectionClass::ExactGeometryWrongObjectPage
            }
            RangeTextInputError::Geometry(ExactGeometryError::NoncontiguousPage { .. }) => {
                RangeResponseRejectionClass::ExactGeometryNoncontiguousPage
            }
            RangeTextInputError::Geometry(ExactGeometryError::PageTooLarge) => {
                RangeResponseRejectionClass::ExactGeometryPageTooLarge
            }
            RangeTextInputError::Geometry(ExactGeometryError::SourceContract) => {
                RangeResponseRejectionClass::ExactGeometrySourceContract
            }
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::InvalidConfiguration,
            )) => RangeResponseRejectionClass::ExactGeometryLayoutInvalidConfiguration,
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::InvalidMetric(_),
            )) => RangeResponseRejectionClass::ExactGeometryLayoutInvalidMetric,
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::InputMismatch,
            )) => RangeResponseRejectionClass::ExactGeometryLayoutInputMismatch,
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::SegmentPolicyMismatch,
            )) => RangeResponseRejectionClass::ExactGeometryLayoutSegmentPolicyMismatch,
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::OutOfOrder,
            )) => RangeResponseRejectionClass::ExactGeometryLayoutOutOfOrder,
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::InvalidPosition,
            )) => RangeResponseRejectionClass::ExactGeometryLayoutInvalidPosition,
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::InvalidSegment,
            )) => RangeResponseRejectionClass::ExactGeometryLayoutInvalidSegment,
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::Ended,
            )) => RangeResponseRejectionClass::ExactGeometryLayoutEnded,
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::CapacityExceeded(_),
            )) => RangeResponseRejectionClass::ExactGeometryLayoutCapacityExceeded,
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::Overflow(_),
            )) => RangeResponseRejectionClass::ExactGeometryLayoutOverflow,
            RangeTextInputError::Geometry(ExactGeometryError::Layout(
                gpui::StreamingLayoutError::Cancelled,
            )) => RangeResponseRejectionClass::ExactGeometryLayoutCancelled,
            RangeTextInputError::IncompleteSurface => {
                RangeResponseRejectionClass::CandidateSurfaceIncomplete
            }
            _ => RangeResponseRejectionClass::OtherDeterministic,
        });
        self.response_rejection_count = self.response_rejection_count.saturating_add(1);
    }

    pub(super) fn retire_page_response_custody(&mut self, key: crate::PageRequestKey) {
        self.response_custody.retain(
            |response| !matches!(response, RangeResponseCustody::Page(page) | RangeResponseCustody::PageNoAliases(page) | RangeResponseCustody::ResidentPage(page) if page.key() == key)
                && !matches!(response, RangeResponseCustody::AliasFanout(fanout) if fanout.page.key() == key),
        );
    }

    pub(super) fn retire_object_response_custody(&mut self, key: crate::ObjectRequestKey) {
        self.response_custody.retain(
            |response| !matches!(response, RangeResponseCustody::Object(page) if page.key() == key),
        );
    }

    pub(super) fn response_custody_storage_charge(&self) -> RangeSurfaceCharge {
        let mut charge = RangeSurfaceCharge {
            bytes: self.response_custody.capacity() * std::mem::size_of::<RangeResponseCustody>(),
            items: self.response_custody.capacity(),
        };
        for response in &self.response_custody {
            let incremental = response
                .incremental_charge()
                .expect("admitted response charge remains representable");
            charge.bytes = charge
                .bytes
                .checked_add(incremental.bytes)
                .expect("admitted response bytes remain representable");
            charge.items = charge
                .items
                .checked_add(incremental.items)
                .expect("admitted response items remain representable");
        }
        charge
    }

    pub(super) fn admit_response_custody(
        &mut self,
        response: RangeResponseCustody,
    ) -> Result<(), RangeResponseCustody> {
        if self.response_custody.len() == self.response_custody.capacity() {
            return Err(response);
        }
        let current = self.current_realization_ownership();
        let Ok(incremental) = response.incremental_charge() else {
            return Err(response);
        };
        let Ok(processing) = response.processing_charge() else {
            return Err(response);
        };
        let peak = RangeSurfaceCharge {
            bytes: match current
                .owned_bytes
                .checked_add(incremental.bytes)
                .and_then(|bytes| bytes.checked_add(processing.bytes))
            {
                Some(bytes) => bytes,
                None => return Err(response),
            },
            items: match current
                .owned_items
                .checked_add(incremental.items)
                .and_then(|items| items.checked_add(processing.items))
            {
                Some(items) => items,
                None => return Err(response),
            },
        };
        if peak.bytes > self.config.limits.max_surface_bytes
            || peak.items > self.config.limits.max_surface_items
        {
            return Err(response);
        }
        self.observe_surface_admission_peak(peak);
        self.response_custody.push_back(response);
        self.observe_realization_ownership();
        Ok(())
    }

    pub(super) fn admit_resident_page_clone(
        &mut self,
        source: crate::PageId,
        key: crate::PageRequestKey,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if self.response_custody.len() == self.response_custody.capacity() {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        let retained = self
            .residency
            .page_by_id(source)
            .ok_or(RangeTextInputError::Stale)?
            .retained_charge();
        let incremental = RangeSurfaceCharge {
            bytes: retained
                .bytes()
                .checked_sub(std::mem::size_of::<crate::RangePage>())
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: retained.items().checked_sub(1).unwrap_or(0),
        };
        let processing = RangeSurfaceCharge {
            bytes: retained.bytes(),
            items: retained.items(),
        };
        let current = self.current_realization_ownership();
        let peak = RangeSurfaceCharge {
            bytes: current
                .owned_bytes
                .checked_add(incremental.bytes)
                .and_then(|bytes| bytes.checked_add(processing.bytes))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: current
                .owned_items
                .checked_add(incremental.items)
                .and_then(|items| items.checked_add(processing.items))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if peak.bytes > self.config.limits.max_surface_bytes
            || peak.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.observe_surface_admission_peak(peak);
        let page = self
            .residency
            .page_by_id(source)
            .and_then(|page| page.clone_for_request(key).ok())
            .ok_or(RangeTextInputError::Stale)?;
        self.response_custody
            .push_back(RangeResponseCustody::ResidentPage(page));
        self.schedule_realization_continuation(cx);
        self.observe_realization_ownership();
        Ok(())
    }

    pub(super) fn admit_borrowed_page_clone(
        &mut self,
        source: &crate::RangePage,
        key: crate::PageRequestKey,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if self.response_custody.len() == self.response_custody.capacity() {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        let retained = source.retained_charge();
        let incremental = RangeSurfaceCharge {
            bytes: retained
                .bytes()
                .checked_sub(std::mem::size_of::<crate::RangePage>())
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: retained.items().checked_sub(1).unwrap_or(0),
        };
        let current = self.current_realization_ownership();
        let peak = RangeSurfaceCharge {
            bytes: current
                .owned_bytes
                .checked_add(incremental.bytes)
                .and_then(|bytes| bytes.checked_add(retained.bytes()))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: current
                .owned_items
                .checked_add(incremental.items)
                .and_then(|items| items.checked_add(retained.items()))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if peak.bytes > self.config.limits.max_surface_bytes
            || peak.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.observe_surface_admission_peak(peak);
        let page = source
            .clone_for_request(key)
            .map_err(|_| RangeTextInputError::Stale)?;
        self.response_custody
            .push_back(RangeResponseCustody::ResidentPage(page));
        self.schedule_realization_continuation(cx);
        self.observe_realization_ownership();
        Ok(())
    }

    pub(super) fn commit_alias_fanout(
        &mut self,
        page: crate::RangePage,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if self.response_custody.len() == self.response_custody.capacity() {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.response_custody
            .push_back(RangeResponseCustody::AliasFanout(AliasFanout {
                page,
                cursor: 0,
                matched: false,
            }));
        self.schedule_realization_continuation(cx);
        self.observe_realization_ownership();
        Ok(())
    }

    pub(super) fn service_response_custody(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> Result<bool, RangeTextInputError> {
        if self.response_custody.is_empty() {
            return Ok(false);
        }
        if !self.try_spend_realization_credit(cx) {
            return Ok(false);
        }
        let Some(response) = self.response_custody.pop_front() else {
            unreachable!("response custody was checked nonempty")
        };
        self.pending_response_exact_geometry_failure_stage = None;
        let retry = response.clone();
        self.active_response_processing = retry
            .transient_charge()
            .expect("admitted response processing charge remains representable");
        self.observe_realization_ownership();
        let result = match response {
            RangeResponseCustody::Page(page) => {
                self.deliver_custodied_page(page, retry.page(), window, cx)
            }
            RangeResponseCustody::PageNoAliases(page) => {
                self.deliver_custodied_page_without_aliases(page, retry.page(), window, cx)
            }
            RangeResponseCustody::ResidentPage(page) => {
                self.deliver_resident_page_continuation(page, window, cx)
            }
            RangeResponseCustody::AliasFanout(fanout) => self.advance_page_alias(fanout, cx),
            RangeResponseCustody::Object(page) => {
                self.deliver_custodied_object_page(page, Some(window), cx)
            }
        };
        self.active_response_processing = RangeSurfaceCharge::default();
        if let Err(error) = result {
            self.record_response_rejection(&retry, &error);
            self.refund_realization_credit();
            let retained = retry.remains_dispatched(self);
            if retained {
                let has_tail = !self.response_custody.is_empty();
                self.response_custody.push_back(retry);
                self.observe_realization_ownership();
                if has_tail || matches!(error, RangeTextInputError::SurfaceCapacity) {
                    self.schedule_realization_continuation(cx);
                }
            } else if !self.response_custody.is_empty() {
                self.schedule_realization_continuation(cx);
            }
            return Err(error);
        }
        if !self.response_custody.is_empty() {
            self.schedule_realization_continuation(cx);
        }
        self.observe_realization_ownership();
        Ok(true)
    }

    pub(super) fn service_object_response_custody(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> Result<bool, RangeTextInputError> {
        if !matches!(self.response_custody.front(), Some(RangeResponseCustody::Object(page)) if !matches!(page.key().purpose(), crate::ObjectPurpose::GeometryIndex | crate::ObjectPurpose::GeometryTarget))
        {
            if self.response_custody.len() > 1 {
                self.schedule_realization_continuation(cx);
            }
            return Ok(false);
        }
        if !self.try_spend_realization_credit(cx) {
            return Ok(false);
        }
        let response = self
            .response_custody
            .pop_front()
            .expect("checked response exists");
        self.pending_response_exact_geometry_failure_stage = None;
        let retry = response.clone();
        let RangeResponseCustody::Object(page) = response else {
            unreachable!()
        };
        self.active_response_processing = retry
            .transient_charge()
            .expect("admitted response processing charge remains representable");
        self.observe_realization_ownership();
        let result = self.deliver_custodied_object_page(page, None, cx);
        self.active_response_processing = RangeSurfaceCharge::default();
        if let Err(error) = result {
            self.record_response_rejection(&retry, &error);
            self.refund_realization_credit();
            let retained = retry.remains_dispatched(self);
            if retained {
                let has_tail = !self.response_custody.is_empty();
                self.response_custody.push_back(retry);
                self.observe_realization_ownership();
                if has_tail || matches!(error, RangeTextInputError::SurfaceCapacity) {
                    self.schedule_realization_continuation(cx);
                }
            } else if !self.response_custody.is_empty() {
                self.schedule_realization_continuation(cx);
            }
            return Err(error);
        }
        if !self.response_custody.is_empty() {
            self.schedule_realization_continuation(cx);
        }
        self.observe_realization_ownership();
        Ok(true)
    }
}
