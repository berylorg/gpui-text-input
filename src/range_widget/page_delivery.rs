use gpui::{Context, Window};
use std::collections::VecDeque;

use super::response_custody::ResponseDeliveryProgress;
use crate::{
    PageDemand, PageFailure, PagePurpose, PageRequest, PageRequestId, PageRequestKey, RangePage,
    RangeTextInput, RangeTextInputError, RangeTextInputRequest,
};

#[derive(Clone, Copy)]
pub(super) struct PendingPageAlias {
    pub(in crate::range_widget) request: PageRequestKey,
    pub(in crate::range_widget) source: PageRequestKey,
}

struct PreparedOrdinaryPageResponseFailure {
    settlement: crate::residency::PreparedRangePageSettlement,
    key: PageRequestKey,
    destination_requests: VecDeque<RangeTextInputRequest>,
}

impl RangeTextInput {
    fn prepare_ordinary_page_response_failure(
        &self,
        settlement: crate::residency::PreparedRangePageSettlement,
        key: PageRequestKey,
    ) -> Result<PreparedOrdinaryPageResponseFailure, RangeTextInputError> {
        let maximum = super::checked_request_capacity(&self.config)
            .expect("constructed range widget retains a valid request capacity");
        let retired_cancel_count = self
            .requests
            .iter()
            .filter(|request| {
                matches!(request, RangeTextInputRequest::CancelPage(cancelled) if *cancelled == key)
            })
            .count();
        if retired_cancel_count > 1 {
            return Err(RangeTextInputError::Stale);
        }
        let required = self
            .requests
            .len()
            .checked_sub(retired_cancel_count)
            .ok_or(RangeTextInputError::SurfaceCapacity)?
            .checked_add(1)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        if required > maximum {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        let destination_requests = VecDeque::with_capacity(maximum);
        if destination_requests.capacity() > maximum {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        Ok(PreparedOrdinaryPageResponseFailure {
            settlement,
            key,
            destination_requests,
        })
    }

    fn commit_ordinary_page_response_failure(
        &mut self,
        prepared: PreparedOrdinaryPageResponseFailure,
    ) {
        let PreparedOrdinaryPageResponseFailure {
            settlement,
            key,
            mut destination_requests,
        } = prepared;
        let prior_requests = std::mem::take(&mut self.requests);
        destination_requests.extend(prior_requests.into_iter().filter(|request| {
            !matches!(request, RangeTextInputRequest::CancelPage(cancelled) if *cancelled == key)
        }));
        destination_requests.push_back(RangeTextInputRequest::ReleasePage(key));
        debug_assert!(destination_requests.len() <= destination_requests.capacity());
        self.requests = destination_requests;
        let settled = self.residency.commit_prepared_settle(settlement);
        debug_assert_eq!(
            settled,
            crate::PageSettlement::Settled(PageFailure::Unavailable)
        );
        assert!(self.dispatched_pages.remove(&key));
    }

    pub(super) fn deliver_resident_page_continuation(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        let result = match page.key().purpose() {
            PagePurpose::PlatformRange => self
                .deliver_platform_page(page, cx)
                .map(|_| ResponseDeliveryProgress::Progressed),
            PagePurpose::Restoration => self
                .deliver_restoration_page(page, cx)
                .map(|_| ResponseDeliveryProgress::Progressed),
            PagePurpose::Segmentation => self
                .deliver_segmentation_page(page, window, cx)
                .map(|_| ResponseDeliveryProgress::Progressed),
            PagePurpose::Clipboard => self
                .deliver_clipboard_page(page, cx)
                .map(|_| ResponseDeliveryProgress::Progressed),
            PagePurpose::Selection => self
                .deliver_replacement_page(page, cx)
                .map(|_| ResponseDeliveryProgress::Progressed),
            _ => Err(RangeTextInputError::Stale),
        };
        Ok(match result {
            Ok(progress) => progress,
            Err(error) => ResponseDeliveryProgress::Rejected(error),
        })
    }

    pub(super) fn accept_page_demand(
        &mut self,
        request: PageRequest,
        demand: PageDemand,
        cx: &mut Context<Self>,
    ) -> Result<Option<RangePage>, RangeTextInputError> {
        match demand {
            PageDemand::Requested(expected) if expected.key() == request.key() => {
                self.push_request(RangeTextInputRequest::Page(request), cx)?;
                Ok(None)
            }
            PageDemand::ResidentAdjacent(page) | PageDemand::ResidentValidation { page, .. } => {
                self.residency
                    .page_by_id(page)
                    .and_then(|page| page.clone_for_request(request.key()).ok())
                    .map(Some)
                    .ok_or(RangeTextInputError::Stale)
            }
            PageDemand::Coalesced(source) => {
                let current = self.current_realization_ownership();
                let required = self
                    .pending_page_aliases
                    .len()
                    .checked_add(1)
                    .ok_or(RangeTextInputError::SurfaceCapacity)?;
                let replacement_charge = if required > self.pending_page_aliases.capacity() {
                    crate::RangeSurfaceCharge {
                        bytes: required
                            .checked_mul(std::mem::size_of::<PendingPageAlias>())
                            .ok_or(RangeTextInputError::SurfaceCapacity)?,
                        items: required,
                    }
                } else {
                    crate::RangeSurfaceCharge::default()
                };
                let peak = crate::RangeSurfaceCharge {
                    bytes: current
                        .owned_bytes
                        .checked_add(replacement_charge.bytes)
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                    items: current
                        .owned_items
                        .checked_add(replacement_charge.items)
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                };
                if peak.bytes > self.config.limits.max_surface_bytes
                    || peak.items > self.config.limits.max_surface_items
                {
                    return Err(RangeTextInputError::SurfaceCapacity);
                }
                self.observe_realization_peak(peak, Some((replacement_charge, required)));
                if required > self.pending_page_aliases.capacity() {
                    self.pending_page_aliases
                        .reserve_exact(required - self.pending_page_aliases.len());
                    debug_assert_eq!(self.pending_page_aliases.capacity(), required);
                }
                self.pending_page_aliases.push(PendingPageAlias {
                    request: request.key(),
                    source,
                });
                self.observe_realization_ownership();
                cx.notify();
                Ok(None)
            }
            _ => Err(RangeTextInputError::Stale),
        }
    }

    pub(super) fn accept_range_continuation_demand(
        &mut self,
        request: PageRequest,
        demand: PageDemand,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        match demand {
            PageDemand::ResidentAdjacent(page) | PageDemand::ResidentValidation { page, .. } => {
                self.admit_resident_page_clone(page, request.key(), cx)
            }
            demand => {
                let resident = self.accept_page_demand(request, demand, cx)?;
                debug_assert!(resident.is_none());
                Ok(())
            }
        }
    }

    fn take_page_alias_at(&mut self, index: usize) -> PageRequestKey {
        let request = self.pending_page_aliases.swap_remove(index).request;
        if self.pending_page_aliases.is_empty() {
            self.pending_page_aliases = Vec::new();
        }
        request
    }

    fn take_one_page_alias(&mut self, source: PageRequestKey) -> Option<PageRequestKey> {
        let index = self
            .pending_page_aliases
            .iter()
            .position(|alias| alias.source == source)?;
        Some(self.take_page_alias_at(index))
    }

    pub(super) fn advance_page_alias(
        &mut self,
        mut fanout: super::response_custody::AliasFanout,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        if fanout.cursor >= self.pending_page_aliases.len() {
            if !fanout.matched {
                self.response_custody.push_back(
                    super::response_custody::RangeResponseCustody::PageNoAliases(fanout.page),
                );
                self.schedule_realization_continuation(cx);
            }
            self.observe_realization_ownership();
            return Ok(ResponseDeliveryProgress::Progressed);
        }
        let pending = self.pending_page_aliases[fanout.cursor];
        if pending.source == fanout.page.key() {
            let required_slots = 1 + usize::from(self.pending_page_aliases.len() > 1);
            if required_slots > self.response_custody.capacity() - self.response_custody.len() {
                return self.settle_page_alias_capacity(fanout, cx);
            }
            if let Err(error) = self.admit_borrowed_page_clone(&fanout.page, pending.request, cx) {
                if matches!(error, RangeTextInputError::SurfaceCapacity) {
                    return self.settle_page_alias_capacity(fanout, cx);
                }
                return Ok(ResponseDeliveryProgress::Rejected(error));
            }
            let retired = self.take_page_alias_at(fanout.cursor);
            debug_assert_eq!(retired, pending.request);
            if !fanout.matched {
                let key = fanout.page.key();
                self.dispatched_pages.remove(&key);
                let _ = self.residency.settle(key, PageFailure::Cancelled);
                self.commit_prepared_request(RangeTextInputRequest::ReleasePage(key));
                fanout.matched = true;
            }
        } else {
            fanout.cursor += 1;
        }
        if fanout.cursor < self.pending_page_aliases.len() {
            self.response_custody.push_back(
                super::response_custody::RangeResponseCustody::AliasFanout(fanout),
            );
        } else if !fanout.matched {
            self.response_custody.push_back(
                super::response_custody::RangeResponseCustody::PageNoAliases(fanout.page),
            );
        }
        self.schedule_realization_continuation(cx);
        self.observe_realization_ownership();
        Ok(ResponseDeliveryProgress::Progressed)
    }

    fn settle_page_alias_capacity(
        &mut self,
        fanout: super::response_custody::AliasFanout,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        let source = fanout.page.key();
        let mut index = 0;
        while index < self.pending_page_aliases.len() {
            if self.pending_page_aliases[index].source != source {
                index += 1;
                continue;
            }
            let alias = self.take_page_alias_at(index);
            let _ = self.residency.settle(alias, PageFailure::Unavailable);
            let _ = self.fail_aliased_page(alias, PageFailure::Unavailable, cx);
        }
        if !fanout.matched {
            let _ = self.residency.settle(source, PageFailure::Unavailable);
            assert!(self.dispatched_pages.remove(&source));
            self.commit_prepared_request(RangeTextInputRequest::ReleasePage(source));
        } else {
            debug_assert!(!self.dispatched_pages.contains(&source));
        }
        self.observe_realization_ownership();
        cx.notify();
        Ok(ResponseDeliveryProgress::AcceptedTerminal(
            RangeTextInputError::SurfaceCapacity,
        ))
    }

    pub fn deliver_page(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::PageResponseRejected(page));
        }
        if !self.dispatched_pages.contains(&page.key()) {
            return Err(RangeTextInputError::PageResponseRejected(page));
        }
        self.admit_response_custody(super::response_custody::RangeResponseCustody::Page(page))
            .map_err(|response| match response {
                super::response_custody::RangeResponseCustody::Page(page) => {
                    RangeTextInputError::PageResponseCapacity(page)
                }
                super::response_custody::RangeResponseCustody::ResidentPage(_)
                | super::response_custody::RangeResponseCustody::PageNoAliases(_)
                | super::response_custody::RangeResponseCustody::AliasFanout(_)
                | super::response_custody::RangeResponseCustody::Object(_) => unreachable!(),
            })?;
        match self.service_response_custody(window, cx) {
            super::response_custody::ResponseCustodyProgress::Idle
            | super::response_custody::ResponseCustodyProgress::Progressed
            | super::response_custody::ResponseCustodyProgress::AcceptedTerminal
            | super::response_custody::ResponseCustodyProgress::RetryableTerminalSurfaceCapacity
            | super::response_custody::ResponseCustodyProgress::RetryableClipboardPreparationCapacity => Ok(()),
            super::response_custody::ResponseCustodyProgress::Rejected(error) => Err(error),
        }
    }

    pub(super) fn deliver_custodied_page(
        &mut self,
        page: RangePage,
        retry: Option<&RangePage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        self.deliver_custodied_page_inner(page, retry, true, window, cx)
    }

    pub(super) fn deliver_custodied_page_without_aliases(
        &mut self,
        page: RangePage,
        retry: Option<&RangePage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        self.deliver_custodied_page_inner(page, retry, false, window, cx)
    }

    fn deliver_custodied_page_inner(
        &mut self,
        page: RangePage,
        retry: Option<&RangePage>,
        check_aliases: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        if !self.mounted {
            if page.key().purpose() == PagePurpose::Restoration {
                self.requests
                    .push_back(RangeTextInputRequest::ReleasePage(page.key()));
                return Ok(ResponseDeliveryProgress::Rejected(
                    RangeTextInputError::Stale,
                ));
            }
            return Ok(ResponseDeliveryProgress::Rejected(
                RangeTextInputError::NotMounted,
            ));
        }
        let key = page.key();
        if !self.dispatched_pages.contains(&key) {
            let _ = self.residency.settle(key, PageFailure::Cancelled);
            self.requests
                .push_back(RangeTextInputRequest::ReleasePage(key));
            return Ok(ResponseDeliveryProgress::Rejected(
                RangeTextInputError::Stale,
            ));
        }
        if key.purpose() == PagePurpose::GeometryIndex {
            return self.deliver_geometry_page_inner(page, true, window, cx);
        }
        if check_aliases && !self.pending_page_aliases.is_empty() {
            if self.response_custody.len() == self.response_custody.capacity() {
                return self.settle_page_alias_capacity(
                    super::response_custody::AliasFanout {
                        page,
                        cursor: 0,
                        matched: false,
                    },
                    cx,
                );
            }
            self.commit_alias_fanout(page, cx)?;
            return Ok(ResponseDeliveryProgress::Progressed);
        }
        if key.purpose() == PagePurpose::GeometryTarget {
            return self.deliver_geometry_target_page_inner(page, true, window, cx);
        }
        let coalesced_clipboard = self.clipboard_waits_on(key).then_some(retry).flatten();
        let mut settled_ordinary_response = false;
        let result = match key.purpose() {
            PagePurpose::GeometryIndex | PagePurpose::GeometryTarget => {
                unreachable!("geometry page purposes were routed before generic delivery")
            }
            PagePurpose::PlatformRange => self
                .deliver_platform_page(page, cx)
                .map(|_| ResponseDeliveryProgress::Progressed),
            PagePurpose::Restoration => self
                .deliver_restoration_page(page, cx)
                .map(|_| ResponseDeliveryProgress::Progressed),
            PagePurpose::Segmentation => self
                .deliver_segmentation_page(page, window, cx)
                .map(|_| ResponseDeliveryProgress::Progressed),
            PagePurpose::Clipboard => self
                .deliver_clipboard_page(page, cx)
                .map(|_| ResponseDeliveryProgress::Progressed),
            PagePurpose::Selection => self
                .deliver_replacement_page(page, cx)
                .map(|_| ResponseDeliveryProgress::Progressed),
            PagePurpose::Viewport | PagePurpose::Caret => {
                let settlement = self
                    .residency
                    .prepare_settle(key, PageFailure::Unavailable)
                    .map_err(|_| RangeTextInputError::Stale)?;
                match self.residency.prepare_admit(page) {
                    Ok(admission) => {
                        self.residency.commit_prepared_admit(admission);
                        Ok(ResponseDeliveryProgress::Progressed)
                    }
                    Err(error) => {
                        let prepared =
                            self.prepare_ordinary_page_response_failure(settlement, key)?;
                        self.commit_ordinary_page_response_failure(prepared);
                        settled_ordinary_response = true;
                        match error {
                            crate::PageAdmissionError::LimitExceeded(_) => {
                                Ok(ResponseDeliveryProgress::AcceptedTerminal(
                                    RangeTextInputError::SurfaceCapacity,
                                ))
                            }
                            _ => Err(RangeTextInputError::Stale),
                        }
                    }
                }
            }
        };
        let retained_clipboard = key.purpose() == PagePurpose::Clipboard
            && self.clipboard.pending_text_page() == Some(key);
        if !retained_clipboard && !settled_ordinary_response {
            self.dispatched_pages.remove(&key);
            self.requests
                .push_back(RangeTextInputRequest::ReleasePage(key));
        }
        let clipboard_service =
            coalesced_clipboard.map(|page| self.service_coalesced_clipboard_page(page, cx));
        let service = self.service_geometry_until_external_boundary(window, cx);
        if coalesced_clipboard.is_some() && matches!(&result, Err(RangeTextInputError::Stale)) {
            clipboard_service.expect("coalesced service exists")?;
            return Ok(match service {
                Ok(()) => ResponseDeliveryProgress::Progressed,
                Err(error) => ResponseDeliveryProgress::AcceptedTerminal(error),
            });
        }
        let result = match result {
            Ok(progress) => progress,
            Err(error) => ResponseDeliveryProgress::Rejected(error),
        };
        if let Some(clipboard_service) = clipboard_service {
            if let Err(error) = clipboard_service {
                return Ok(ResponseDeliveryProgress::Rejected(error));
            }
        }
        Ok(match service {
            Ok(()) => result,
            Err(error) => ResponseDeliveryProgress::AcceptedTerminal(error),
        })
    }

    pub(super) fn deliver_segmentation_page(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self
            .segmentation
            .as_ref()
            .is_some_and(|continuation| *continuation.pending_request() == page.key())
        {
            return Err(RangeTextInputError::Stale);
        }
        let mut continuation = self.segmentation.take().ok_or(RangeTextInputError::Stale)?;
        let old_key = *continuation.pending_request();
        let continuation_direction = continuation.direction();
        let binding = self.config.binding;
        let page_bytes = self.config.limits.page_bytes;
        let mut next_id = self.next_id;
        let resumed = continuation.resume(&page, |adjacent| {
            let (anchor, direction) = match adjacent.edge() {
                crate::AdjacentPageEdge::NextChunk(offset) => {
                    (offset, crate::PageDirection::Forward)
                }
                crate::AdjacentPageEdge::PrevChunk(offset)
                | crate::AdjacentPageEdge::PreContext(offset) => {
                    (offset, crate::PageDirection::Backward)
                }
                crate::AdjacentPageEdge::Replay(offset) => (
                    offset,
                    match continuation_direction {
                        crate::SegmentationDirection::Forward => crate::PageDirection::Forward,
                        crate::SegmentationDirection::Reverse => crate::PageDirection::Backward,
                    },
                ),
            };
            let id = PageRequestId::new(next_id);
            next_id = next_id.checked_add(1).expect("range widget id exhausted");
            crate::PageRequestKey::adjacent(
                id,
                binding.binding(),
                binding.revision(),
                PagePurpose::Segmentation,
                anchor,
                direction,
                page_bytes,
            )
            .expect("widget limits already validate adjacent page ceiling")
        });
        self.next_id = next_id;
        let _ = self.residency.settle(old_key, PageFailure::Cancelled);
        let resumed = resumed.map_err(|_| RangeTextInputError::Stale)?;
        match resumed {
            crate::SegmentationResume::NeedPage => {
                let key = *continuation.pending_request();
                let demand = self
                    .residency
                    .demand(key.id(), PagePurpose::Segmentation, key.demand())
                    .map_err(|_| RangeTextInputError::Busy)?;
                self.segmentation = Some(continuation);
                if let Err(error) =
                    self.accept_range_continuation_demand(crate::PageRequest::new(key), demand, cx)
                {
                    self.segmentation = None;
                    self.segmentation_action = None;
                    return Err(error);
                }
            }
            crate::SegmentationResume::Complete(boundary) => {
                let action = self
                    .segmentation_action
                    .take()
                    .ok_or(RangeTextInputError::Stale)?;
                self.apply_boundary(boundary.offset(), action, window, cx)?;
            }
        }
        cx.notify();
        Ok(())
    }

    pub fn fail_page(
        &mut self,
        key: crate::PageRequestKey,
        failure: PageFailure,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.dispatched_pages.remove(&key) {
            return Err(RangeTextInputError::Stale);
        }
        let _ = self.residency.settle(key, failure);
        if self
            .pending_page_aliases
            .iter()
            .any(|alias| alias.source == key)
        {
            while let Some(request) = self.take_one_page_alias(key) {
                self.fail_aliased_page(request, failure, cx)?;
            }
            return Ok(());
        }
        if self.clipboard_waits_on(key) {
            return self.fail_coalesced_clipboard_page(key, failure, cx);
        }
        if let Some(job) = self.active_geometry {
            let coalesced_wait = self.geometry_waits_on(key)
                && self
                    .pending_geometry_page
                    .as_ref()
                    .is_some_and(|pending| pending.job == job);
            let release = if coalesced_wait {
                Some(
                    self.geometry
                        .cancel(job)
                        .map_err(RangeTextInputError::Geometry)?,
                )
            } else {
                self.geometry.fail_page(job, key).ok()
            };
            if let Some(release) = release {
                self.release_geometry(
                    &release,
                    release.pages.contains(&key).then_some(key),
                    None,
                    Some(cx),
                );
                self.active_geometry = None;
                self.pending_index_intent = false;
                self.reject_restoration_geometry(cx)?;
                return Ok(());
            }
        }
        if self
            .platform
            .as_ref()
            .is_some_and(|replay| replay.pending_key() == key)
        {
            self.platform = None;
            return Ok(());
        }
        if self
            .restoration
            .as_ref()
            .is_some_and(|validation| validation.pending_text() == Some(key))
        {
            self.reject_restoration(cx);
            return Ok(());
        }
        if self
            .replacement
            .as_ref()
            .is_some_and(|replacement| replacement.pending() == key)
        {
            self.replacement = None;
            return Ok(());
        }
        if self
            .segmentation
            .as_ref()
            .is_some_and(|continuation| *continuation.pending_request() == key)
        {
            self.segmentation = None;
            self.segmentation_action = None;
            return Ok(());
        }
        if key.purpose() == PagePurpose::Clipboard {
            let progress = self
                .clipboard
                .settle_text_page(key, failure)
                .map_err(|_| RangeTextInputError::Stale)?;
            return self.advance_clipboard(progress, cx);
        }
        Err(RangeTextInputError::Stale)
    }

    fn fail_aliased_page(
        &mut self,
        key: PageRequestKey,
        failure: PageFailure,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if self
            .platform
            .as_ref()
            .is_some_and(|replay| replay.pending_key() == key)
        {
            self.platform = None;
            return Ok(());
        }
        if self
            .restoration
            .as_ref()
            .is_some_and(|validation| validation.pending_text() == Some(key))
        {
            self.reject_restoration(cx);
            return Ok(());
        }
        if self
            .replacement
            .as_ref()
            .is_some_and(|replacement| replacement.pending() == key)
        {
            self.replacement = None;
            return Ok(());
        }
        if self
            .segmentation
            .as_ref()
            .is_some_and(|continuation| *continuation.pending_request() == key)
        {
            self.segmentation = None;
            self.segmentation_action = None;
            return Ok(());
        }
        if key.purpose() == PagePurpose::Clipboard {
            let progress = self
                .clipboard
                .settle_text_page(key, failure)
                .map_err(|_| RangeTextInputError::Stale)?;
            return self.advance_clipboard(progress, cx);
        }
        Err(RangeTextInputError::Stale)
    }
}
