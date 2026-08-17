use gpui::{Context, Window};

use crate::{
    PageDemand, PageFailure, PagePurpose, PageRequest, PageRequestId, PageRequestKey, RangePage,
    RangeTextInput, RangeTextInputError, RangeTextInputRequest,
};

pub(super) struct PendingPageAlias {
    request: PageRequestKey,
    source: PageRequestKey,
}

impl RangeTextInput {
    pub(super) fn accept_page_demand(
        &mut self,
        request: PageRequest,
        demand: PageDemand,
        cx: &mut Context<Self>,
    ) -> Result<Option<RangePage>, RangeTextInputError> {
        match demand {
            PageDemand::Requested(expected) if expected.key() == request.key() => {
                self.push_request(RangeTextInputRequest::Page(request), cx);
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
                self.pending_page_aliases.push(PendingPageAlias {
                    request: request.key(),
                    source,
                });
                cx.notify();
                Ok(None)
            }
            _ => Err(RangeTextInputError::Stale),
        }
    }

    fn take_page_aliases(&mut self, source: PageRequestKey) -> Vec<PageRequestKey> {
        let mut requests = Vec::new();
        let mut index = 0;
        while index < self.pending_page_aliases.len() {
            if self.pending_page_aliases[index].source == source {
                requests.push(self.pending_page_aliases.remove(index).request);
            } else {
                index += 1;
            }
        }
        requests
    }

    fn deliver_aliased_page(
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
    pub fn deliver_page(
        &mut self,
        page: RangePage,
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
            return self.deliver_geometry_page(page, window, cx);
        }
        let aliases = self.take_page_aliases(key);
        if !aliases.is_empty() {
            let pages = aliases
                .into_iter()
                .map(|request| page.clone_for_request(request))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| RangeTextInputError::Stale)?;
            self.dispatched_pages.remove(&key);
            let _ = self.residency.settle(key, PageFailure::Cancelled);
            self.requests
                .push_back(RangeTextInputRequest::ReleasePage(key));
            for page in pages {
                self.deliver_aliased_page(page, window, cx)?;
            }
            return self.service_geometry_until_external_boundary(window, cx);
        }
        if key.purpose() == PagePurpose::GeometryTarget {
            return self.deliver_geometry_target_page(page, window, cx);
        }
        let coalesced_clipboard = self.clipboard_waits_on(key).then(|| page.clone());
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
        let clipboard_service = coalesced_clipboard
            .as_ref()
            .map(|page| self.service_coalesced_clipboard_page(page, cx));
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
                let resident = self.accept_page_demand(crate::PageRequest::new(key), demand, cx)?;
                if let Some(page) = resident {
                    self.deliver_segmentation_page(page, window, cx)?;
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
        let aliases = self.take_page_aliases(key);
        if !aliases.is_empty() {
            for request in aliases {
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
