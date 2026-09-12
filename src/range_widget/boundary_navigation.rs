use gpui::Context;

use super::{RangeSourceSelection, RangeTextInput, RangeTextInputError, RangeTextInputRequest};
use crate::{
    ByteOffset, InlineObjectGap, ObjectDemandEnvelope, ObjectDirection, ObjectPage, ObjectPurpose,
    ObjectRequest, ObjectRequestId, ObjectRequestKey, PresentationGeneration, RangeBinding,
    SegmentationDirection, SourcePosition,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct PendingBoundaryMove {
    binding: RangeBinding,
    presentation: PresentationGeneration,
    selection: RangeSourceSelection,
    offset: ByteOffset,
    direction: SegmentationDirection,
    extend: bool,
    request: Option<ObjectRequestKey>,
    endpoint: Option<SourcePosition>,
}

impl RangeTextInput {
    pub(super) fn boundary_segmentation_request(&self) -> Option<crate::PageRequestKey> {
        matches!(
            self.segmentation_action,
            Some(super::interaction::PendingBoundaryAction::Move { .. })
        )
        .then(|| {
            self.segmentation
                .as_ref()
                .map(|continuation| *continuation.pending_request())
        })
        .flatten()
    }
    pub(super) fn boundary_response_waits_for_release_slot(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        let blocked = self.requests.len() == self.requests.capacity()
            && matches!(self.response_custody.front(), Some(super::response_custody::RangeResponseCustody::Object(page)) if page.key().purpose() == ObjectPurpose::Selection);
        if blocked {
            self.schedule_realization_continuation(cx);
        }
        blocked
    }
    pub(super) fn boundary_move_request(&self) -> Option<ObjectRequestKey> {
        self.pending_boundary_move
            .and_then(|pending| pending.request)
    }

    pub(super) fn retire_boundary_segmentation(&mut self) {
        if let Some(key) = self.boundary_segmentation_request() {
            self.retire_page_response_custody(key);
            self.dispatched_pages.remove(&key);
            self.requests.retain(|request| !matches!(request, RangeTextInputRequest::Page(page) if page.key() == key));
            let _ = self.residency.cancel(key);
            self.segmentation = None;
            self.segmentation_action = None;
        }
    }

    pub(super) fn retire_boundary_move(&mut self) {
        if let Some(key) = self.boundary_move_request() {
            self.retire_object_response_custody(key);
            self.dispatched_object_pages.remove(&key);
            self.requests.retain(|request| !matches!(request, RangeTextInputRequest::ObjectPage(page) if page.key() == key));
        }
        self.pending_boundary_move = None;
    }
    pub(super) fn retain_boundary_move(
        &mut self,
        offset: ByteOffset,
        direction: SegmentationDirection,
        extend: bool,
        selection: RangeSourceSelection,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        self.pending_boundary_move = Some(PendingBoundaryMove {
            binding: self.config.binding,
            presentation: self.config.presentation_generation,
            selection,
            offset,
            direction,
            extend,
            request: None,
            endpoint: None,
        });
        self.service_pending_boundary_move(cx)
    }

    pub(super) fn cancel_boundary_move(&mut self) {
        if let Some(pending) = self.pending_boundary_move.take() {
            if let Some(key) = pending.request {
                self.cancel_object_page_dispatch(key);
            }
        }
    }

    fn boundary_move_is_current(&self, pending: PendingBoundaryMove) -> bool {
        self.enabled
            && self.mounted
            && self.config.binding == pending.binding
            && self.config.presentation_generation == pending.presentation
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.selection() == pending.selection)
            && self
                .target_intent_desired()
                .source_selection
                .is_none_or(|selection| selection == pending.selection)
    }

    pub(super) fn service_pending_boundary_move(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let Some(mut pending) = self.pending_boundary_move else {
            return Ok(());
        };
        if !self.boundary_move_is_current(pending) {
            if pending
                .request
                .is_some_and(|key| self.dispatched_object_pages.contains(&key))
                && self.requests.len() == self.requests.capacity()
            {
                self.schedule_realization_continuation(cx);
                return Ok(());
            }
            self.cancel_boundary_move();
            return Ok(());
        }
        if pending.request.is_some() {
            return Ok(());
        }
        if let Some(head) = pending.endpoint {
            let Some(surface) = self.interactive_surface() else {
                self.schedule_realization_continuation(cx);
                return Ok(());
            };
            let selection = RangeSourceSelection {
                anchor: if pending.extend {
                    pending.selection.anchor
                } else {
                    head
                },
                head,
            };
            let selected_object = surface.object_selected_by(selection);
            self.pending_boundary_move = None;
            return self.publish_source_selection(selection, selected_object, None, cx);
        }
        if !self.try_spend_realization_credit(cx) {
            return Ok(());
        }
        let demand = ObjectDemandEnvelope::anchor(
            pending.offset,
            None,
            match pending.direction {
                SegmentationDirection::Forward => ObjectDirection::Forward,
                SegmentationDirection::Reverse => ObjectDirection::Backward,
            },
            self.config.clipboard_limits.max_object_page_objects(),
            self.config
                .clipboard_limits
                .max_object_page_retained_bytes(),
        )
        .map_err(|_| RangeTextInputError::InvalidLimits)?;
        let key = ObjectRequestKey::new(
            ObjectRequestId::new(self.next_id()),
            pending.binding.binding(),
            pending.binding.revision(),
            pending.presentation,
            ObjectPurpose::Selection,
            demand,
        )
        .map_err(|_| RangeTextInputError::InvalidLimits)?;
        if let Err(error) = self.push_request(
            RangeTextInputRequest::ObjectPage(ObjectRequest::new(key)),
            cx,
        ) {
            self.refund_realization_credit();
            self.schedule_realization_continuation(cx);
            return match error {
                RangeTextInputError::SurfaceCapacity => Ok(()),
                error => Err(error),
            };
        }
        pending.request = Some(key);
        self.pending_boundary_move = Some(pending);
        Ok(())
    }

    pub(super) fn deliver_boundary_move_page(
        &mut self,
        page: ObjectPage,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        self.commit_prepared_request(RangeTextInputRequest::ReleaseObjectPage(key));
        let Some(mut pending) = self.pending_boundary_move else {
            return Err(RangeTextInputError::Stale);
        };
        if pending.request != Some(key) {
            return Err(RangeTextInputError::Stale);
        }
        if !self.boundary_move_is_current(pending) {
            self.pending_boundary_move = None;
            return Err(RangeTextInputError::Stale);
        }
        let endpoint = boundary_endpoint(&page, pending.offset, pending.direction);
        let Some(endpoint) = endpoint else {
            self.pending_boundary_move = None;
            return Err(RangeTextInputError::Stale);
        };
        pending.request = None;
        pending.endpoint = Some(endpoint);
        self.pending_boundary_move = Some(pending);
        self.service_pending_boundary_move(cx)
    }

    pub(super) fn fail_boundary_move_page(
        &mut self,
        key: ObjectRequestKey,
    ) -> Result<(), RangeTextInputError> {
        if self
            .pending_boundary_move
            .is_some_and(|pending| pending.request == Some(key))
        {
            self.pending_boundary_move = None;
            Ok(())
        } else {
            Err(RangeTextInputError::Stale)
        }
    }
}

pub(super) fn boundary_endpoint(
    page: &ObjectPage,
    offset: ByteOffset,
    direction: SegmentationDirection,
) -> Option<SourcePosition> {
    let object = match direction {
        SegmentationDirection::Forward => page.objects().first(),
        SegmentationDirection::Reverse => page.objects().last(),
    };
    let gap = match object {
        Some(object) if object.anchor() == offset => {
            let neighbor = crate::InlineObjectNeighbor::new(object.id(), object.order());
            match direction {
                SegmentationDirection::Forward => InlineObjectGap::Before(neighbor),
                SegmentationDirection::Reverse => InlineObjectGap::After(neighbor),
            }
        }
        Some(_) => return None,
        None => InlineObjectGap::NoObjects,
    };
    let position = SourcePosition::new(offset, gap);
    crate::object_residency::page_proves_gap(page, position).then_some(position)
}
