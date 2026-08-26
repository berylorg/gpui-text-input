use gpui::{Context, Window};

use crate::{
    PageDemand, PageFailure, PagePurpose, PageRequest, PageRequestId, PageRequestKey, RangePage,
    RangeTextInput, RangeTextInputError, RangeTextInputRequest,
};

#[derive(Clone, Copy)]
pub(super) struct PendingPageAlias {
    pub(in crate::range_widget) request: PageRequestKey,
    pub(in crate::range_widget) source: PageRequestKey,
}

impl RangeTextInput {
    pub(super) fn range_continuation_waits_on(&self, key: PageRequestKey) -> bool {
        self.segmentation
            .as_ref()
            .is_some_and(|continuation| continuation.pending_request() == &key)
            || self
                .platform
                .as_ref()
                .is_some_and(|replay| replay.pending_key() == key)
            || self
                .replacement
                .as_ref()
                .is_some_and(|scan| scan.pending() == key)
            || self.clipboard.pending_text_page() == Some(key)
            || self
                .restoration
                .as_ref()
                .is_some_and(|validation| validation.pending_text == Some(key))
    }

    pub(super) fn deliver_resident_page_continuation(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        match page.key().purpose() {
            PagePurpose::PlatformRange => self.deliver_platform_page(page, cx),
            PagePurpose::Restoration => self.deliver_restoration_page(page, cx),
            PagePurpose::Segmentation => self.deliver_segmentation_page(page, window, cx),
            PagePurpose::Clipboard => self.deliver_clipboard_page(page, cx),
            PagePurpose::Selection => self.deliver_replacement_page(page, cx),
            _ => Err(RangeTextInputError::Stale),
        }
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
    ) -> Result<(), RangeTextInputError> {
        if fanout.cursor >= self.pending_page_aliases.len() {
            if !fanout.matched {
                self.response_custody.push_back(
                    super::response_custody::RangeResponseCustody::PageNoAliases(fanout.page),
                );
                self.schedule_realization_continuation(cx);
            }
            self.observe_realization_ownership();
            return Ok(());
        }
        let pending = self.pending_page_aliases[fanout.cursor];
        if pending.source == fanout.page.key() {
            let required_slots = 1 + usize::from(self.pending_page_aliases.len() > 1);
            if required_slots > self.response_custody.capacity() - self.response_custody.len() {
                return Err(RangeTextInputError::SurfaceCapacity);
            }
            self.admit_borrowed_page_clone(&fanout.page, pending.request, cx)?;
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
        Ok(())
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
        let key = page.key();
        if !self.dispatched_pages.contains(&key) {
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
            Ok(_) => Ok(()),
            Err(_) if self.dispatched_pages.contains(&key) => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub(super) fn deliver_custodied_page(
        &mut self,
        page: RangePage,
        retry: Option<&RangePage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        self.deliver_custodied_page_inner(page, retry, true, window, cx)
    }

    pub(super) fn deliver_custodied_page_without_aliases(
        &mut self,
        page: RangePage,
        retry: Option<&RangePage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        self.deliver_custodied_page_inner(page, retry, false, window, cx)
    }

    fn deliver_custodied_page_inner(
        &mut self,
        page: RangePage,
        retry: Option<&RangePage>,
        check_aliases: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            if page.key().purpose() == PagePurpose::Restoration {
                self.requests
                    .push_back(RangeTextInputRequest::ReleasePage(page.key()));
                return Err(RangeTextInputError::Stale);
            }
            return Err(RangeTextInputError::NotMounted);
        }
        let key = page.key();
        if !self.dispatched_pages.contains(&key) {
            let _ = self.residency.settle(key, PageFailure::Cancelled);
            self.requests
                .push_back(RangeTextInputRequest::ReleasePage(key));
            return Err(RangeTextInputError::Stale);
        }
        if key.purpose() == PagePurpose::GeometryIndex {
            return self.deliver_geometry_page_inner(page, true, window, cx);
        }
        if check_aliases && !self.pending_page_aliases.is_empty() {
            if self.response_custody.len() == self.response_custody.capacity() {
                return Err(RangeTextInputError::SurfaceCapacity);
            }
            self.commit_alias_fanout(page, cx)?;
            return self.service_geometry_until_external_boundary(window, cx);
        }
        if key.purpose() == PagePurpose::GeometryTarget {
            return self.deliver_geometry_target_page_inner(page, true, window, cx);
        }
        let coalesced_clipboard = self.clipboard_waits_on(key).then_some(retry).flatten();
        let result = match key.purpose() {
            PagePurpose::GeometryIndex | PagePurpose::GeometryTarget => {
                unreachable!("geometry page purposes were routed before generic delivery")
            }
            PagePurpose::PlatformRange => self.deliver_platform_page(page, cx),
            PagePurpose::Restoration => self.deliver_restoration_page(page, cx),
            PagePurpose::Segmentation => self.deliver_segmentation_page(page, window, cx),
            PagePurpose::Clipboard => self.deliver_clipboard_page(page, cx),
            PagePurpose::Selection => self.deliver_replacement_page(page, cx),
            PagePurpose::Viewport | PagePurpose::Caret => {
                self.residency
                    .admit(page)
                    .map_err(|_| RangeTextInputError::Stale)?;
                Ok(())
            }
        };
        self.dispatched_pages.remove(&key);
        self.requests
            .push_back(RangeTextInputRequest::ReleasePage(key));
        let clipboard_service =
            coalesced_clipboard.map(|page| self.service_coalesced_clipboard_page(page, cx));
        let service = self.service_geometry_until_external_boundary(window, cx);
        if coalesced_clipboard.is_some() && matches!(&result, Err(RangeTextInputError::Stale)) {
            clipboard_service.expect("coalesced service exists")?;
            return service;
        }
        result?;
        if let Some(clipboard_service) = clipboard_service {
            clipboard_service?;
        }
        service
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
        if self.geometry_waits_on(key) {
            cx.notify();
            return Ok(());
        }
        if let Some(job) = self.active_geometry {
            if let Ok(release) = self.geometry.fail_page(job, key) {
                self.release_geometry(&release, Some(key), None, Some(cx));
                self.active_geometry = None;
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
