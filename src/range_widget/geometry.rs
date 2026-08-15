//! Exact geometry lifecycle and coherent-surface publication.

use std::{collections::VecDeque, mem::size_of};

use gpui::{Context, Window};

use super::surface::PreparedCoherentRangeSurface;
use super::{
    CoherentRangeSurface, DesiredSurface, RangeScrollAnchor, RangeTextInput, RangeTextInputError,
    RangeTextInputRequest, SurfaceCandidate,
};
use crate::{
    ExactGeometryProgress, ObjectDemand, ObjectPageId, ObjectRequestId, ObjectRequestKey,
    PageDemand, PageFailure, PageId, PageRequest, PageRequestId, PageRequestKey, RangePage,
    RangeTextInputEvent,
};

pub(super) struct PendingGeometryPage {
    pub(super) job: crate::GeometryJobKey,
    pub(super) request: PageRequest,
    pub(super) wait: GeometryPageWait,
}

pub(super) struct PendingGeometryObject {
    pub(super) job: crate::GeometryJobKey,
    pub(super) request: crate::ObjectRequest,
    pub(super) text_page: PageId,
    pub(super) wait: GeometryObjectWait,
}

pub(super) struct PreparedTargetPublication {
    state: SurfaceCandidate,
    surface: PreparedCoherentRangeSurface,
    resident_payload_charge: crate::RangeSurfaceCharge,
    prepared_allocation_charge: crate::RangeSurfaceCharge,
    pages: Vec<crate::RangePage>,
    object_pages: Vec<crate::ObjectPage>,
    select_all: Option<crate::RangeSourceSelection>,
    active_loss: Option<crate::InlineObjectRealizationLoss>,
    active_result: Option<Option<super::ActiveInlineObject>>,
    activation: Option<crate::InlineObjectActivation>,
}

struct PreparedTerminalResponsePublication {
    geometry: crate::range_geometry::PreparedTargetResponse,
    text_admission: Option<crate::residency::PreparedRangePageAdmission>,
    object_admission: Option<crate::object_residency::PreparedObjectPageAdmission>,
    text_touch: Option<PageId>,
    object_touch: Option<ObjectPageId>,
    publication: PreparedTargetPublication,
    requests: VecDeque<RangeTextInputRequest>,
    release_request: Option<RangeTextInputRequest>,
    completed_page: Option<PageRequestKey>,
    completed_object_page: Option<ObjectRequestKey>,
    admission_charge: crate::RangeSurfaceCharge,
}

struct PreparedNonterminalResponsePublication {
    geometry: crate::range_geometry::PreparedTargetResponse,
    text_admission: Option<crate::residency::PreparedRangePageAdmission>,
    object_admission: Option<crate::object_residency::PreparedObjectPageAdmission>,
    text_demand: Option<crate::residency::PreparedPageDemand>,
    object_demand: Option<crate::object_residency::PreparedObjectDemand>,
    text_touches: [Option<PageId>; 2],
    object_touches: [Option<ObjectPageId>; 2],
    pending_page: Option<PendingGeometryPage>,
    pending_object: Option<PendingGeometryObject>,
    requests: VecDeque<RangeTextInputRequest>,
    effects: [Option<RangeTextInputRequest>; 2],
    completed_page: Option<PageRequestKey>,
    completed_object_page: Option<ObjectRequestKey>,
    next_id: u64,
}

pub(super) enum TerminalTargetPreparation {
    Retarget(DesiredSurface),
    Publication(PreparedTargetPublication),
}

impl PreparedTargetPublication {
    /// Initialized resident page records, their semantic facts, and retained payloads.
    ///
    /// This excludes the destination transfer-vector slots: those are distinct allocations that
    /// coexist with these initialized records until commit moves each page into its prepared slot.
    pub(super) const fn resident_payload_charge(&self) -> crate::RangeSurfaceCharge {
        self.resident_payload_charge
    }

    /// Candidate-owned surface boxes and empty resident-page transfer-vector slots.
    ///
    /// The terminal target is excluded because the prepared geometry transition owns and charges
    /// it. Resident page records and payloads are excluded because `resident_payload_charge`
    /// represents their existing allocation exactly once.
    pub(super) const fn prepared_allocation_charge(&self) -> crate::RangeSurfaceCharge {
        self.prepared_allocation_charge
    }

    pub(super) const fn final_charge(&self) -> crate::RangeSurfaceCharge {
        self.surface.charge()
    }

    pub(super) const fn active_loss(&self) -> Option<crate::InlineObjectRealizationLoss> {
        self.active_loss
    }

    pub(super) const fn activation(&self) -> Option<crate::InlineObjectActivation> {
        self.activation
    }
}

fn resolve_prepared_active_object(
    current: Option<super::ActiveInlineObject>,
    desired: DesiredSurface,
    surface: &PreparedCoherentRangeSurface,
) -> Result<
    (
        Option<Option<super::ActiveInlineObject>>,
        Option<crate::InlineObjectRealizationLoss>,
        Option<crate::InlineObjectActivation>,
    ),
    RangeTextInputError,
> {
    match desired.inline_object_interaction {
        Some(super::DesiredInlineObjectInteraction::Set {
            object_id,
            order,
            activation_eligible,
            origin,
        }) => {
            let object = surface
                .object_selected_by(surface.selection())
                .filter(|object| object.id() == object_id && object.order() == order)
                .ok_or(RangeTextInputError::IncompleteSurface)?;
            let key = surface.geometry_key();
            let active = super::ActiveInlineObject {
                anchor: crate::RealizedInlineObjectAnchor {
                    binding: surface.binding(),
                    object_id,
                    order,
                    presentation_generation: key.presentation_generation(),
                    layout_epoch: key.epoch(),
                    bounds: object.bounds(),
                },
                leading: object.leading(),
                trailing: object.trailing(),
                activation_eligible,
            };
            let loss = current
                .filter(|prior| prior.anchor != active.anchor)
                .map(|prior| crate::InlineObjectRealizationLoss {
                    anchor: prior.anchor,
                    reason: crate::InlineObjectRealizationLossReason::SelectionChanged,
                });
            let activation = origin.filter(|_| activation_eligible).map(|origin| {
                crate::InlineObjectActivation {
                    anchor: active.anchor,
                    origin,
                }
            });
            Ok((Some(Some(active)), loss, activation))
        }
        Some(super::DesiredInlineObjectInteraction::Clear(reason)) => Ok((
            Some(None),
            current.map(|active| crate::InlineObjectRealizationLoss {
                anchor: active.anchor,
                reason,
            }),
            None,
        )),
        None => {
            let loss = current.and_then(|active| {
                let key = surface.geometry_key();
                let same_key = surface.binding() == active.anchor.binding
                    && key.presentation_generation() == active.anchor.presentation_generation
                    && key.epoch() == active.anchor.layout_epoch;
                let still_realized =
                    surface
                        .object_selected_by(surface.selection())
                        .is_some_and(|object| {
                            object.id() == active.anchor.object_id
                                && object.order() == active.anchor.order
                                && object.bounds() == active.anchor.bounds
                        });
                (!same_key || !still_realized).then(|| crate::InlineObjectRealizationLoss {
                    anchor: active.anchor,
                    reason: if same_key {
                        crate::InlineObjectRealizationLossReason::Unrealized
                    } else {
                        crate::InlineObjectRealizationLossReason::Superseded
                    },
                })
            });
            Ok((loss.map(|_| None), loss, None))
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum GeometryObjectWait {
    Resident(ObjectPageId),
    Coalesced(ObjectRequestKey),
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum GeometryPageWait {
    Resident(PageId),
    Coalesced(PageRequestKey),
}

impl RangeTextInput {
    fn target_response_successor(
        &self,
    ) -> Result<crate::range_geometry::TargetResponseSuccessor, RangeTextInputError> {
        self.next_id
            .checked_add(1)
            .ok_or(RangeTextInputError::Busy)?;
        Ok(crate::range_geometry::TargetResponseSuccessor {
            page_id: PageRequestId::new(self.next_id),
            object_id: ObjectRequestId::new(self.next_id),
            max_objects: self.config.object_residency_limits.max_resident_objects(),
            max_object_bytes: self.config.object_residency_limits.max_resident_bytes(),
        })
    }

    pub(super) fn prepare_terminal_target_publication(
        &self,
        geometry: &crate::range_geometry::PreparedGeometryTransition,
        state: SurfaceCandidate,
    ) -> Result<TerminalTargetPreparation, RangeTextInputError> {
        let target = geometry
            .terminal_target()
            .ok_or(RangeTextInputError::Stale)?;
        if state.job != geometry.key() {
            return Err(RangeTextInputError::Stale);
        }
        self.prepare_target_publication_from(
            target,
            state,
            self.residency.resident_page_iter(),
            self.object_residency.resident_page_iter(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_nonterminal_response_publication(
        &self,
        geometry: crate::range_geometry::PreparedTargetResponse,
        text_admission: Option<crate::residency::PreparedRangePageAdmission>,
        object_admission: Option<crate::object_residency::PreparedObjectPageAdmission>,
        text_touch: Option<PageId>,
        object_touch: Option<ObjectPageId>,
        consumed_page: Option<PageRequestKey>,
        consumed_object_page: Option<ObjectRequestKey>,
        completed_page: Option<PageRequestKey>,
        completed_object_page: Option<ObjectRequestKey>,
    ) -> Result<PreparedNonterminalResponsePublication, RangeTextInputError> {
        if geometry.progress() == ExactGeometryProgress::TargetComplete
            || geometry.release().pages.as_slice() != consumed_page.as_slice()
            || geometry.release().object_pages.as_slice() != consumed_object_page.as_slice()
            || !geometry.release().jobs.is_empty()
            || completed_page.is_some() != text_admission.is_some()
            || completed_object_page.is_some() != object_admission.is_some()
        {
            return Err(RangeTextInputError::Stale);
        }
        let job = geometry.key();
        let successor = geometry.successor().ok_or(RangeTextInputError::Stale)?;
        let retired_pages = completed_page.as_slice();
        let retired_object_pages = completed_object_page.as_slice();
        let mut text_touches = [text_touch, None];
        let mut object_touches = [object_touch, None];
        let mut pending_page = None;
        let mut pending_object = None;
        let (text_demand, object_demand, request_effect) = match successor {
            crate::range_geometry::PreparedTargetSuccessor::Page(request) => {
                let prepared = if let Some(admission) = text_admission.as_ref() {
                    self.residency.prepare_demand_after_retirement_from(
                        request.key().id(),
                        request.key().purpose(),
                        request.key().demand(),
                        retired_pages,
                        admission.projected_resident_pages(&self.residency),
                    )
                } else {
                    self.residency.prepare_demand_after_retirement_from(
                        request.key().id(),
                        request.key().purpose(),
                        request.key().demand(),
                        retired_pages,
                        self.residency.resident_page_iter(),
                    )
                }
                .map_err(|_| RangeTextInputError::Busy)?;
                let effect = match prepared.outcome() {
                    PageDemand::Requested(expected) if expected == request => {
                        Some(RangeTextInputRequest::Page(request))
                    }
                    PageDemand::ResidentAdjacent(page) => {
                        text_touches[1] = Some(page);
                        pending_page = Some(PendingGeometryPage {
                            job,
                            request,
                            wait: GeometryPageWait::Resident(page),
                        });
                        None
                    }
                    PageDemand::Coalesced(existing) => {
                        pending_page = Some(PendingGeometryPage {
                            job,
                            request,
                            wait: GeometryPageWait::Coalesced(existing),
                        });
                        None
                    }
                    _ => return Err(RangeTextInputError::Stale),
                };
                (Some(prepared), None, effect)
            }
            crate::range_geometry::PreparedTargetSuccessor::Object { request, text_page } => {
                let prepared = if let Some(admission) = object_admission.as_ref() {
                    self.object_residency.prepare_demand_after_retirement_from(
                        request.key().id(),
                        request.key().purpose(),
                        request.key().demand(),
                        retired_object_pages,
                        admission.projected_resident_pages(&self.object_residency),
                    )
                } else {
                    self.object_residency.prepare_demand_after_retirement_from(
                        request.key().id(),
                        request.key().purpose(),
                        request.key().demand(),
                        retired_object_pages,
                        self.object_residency.resident_page_iter(),
                    )
                }
                .map_err(|_| RangeTextInputError::Busy)?;
                let (wait, effect) = match prepared.outcome() {
                    ObjectDemand::Requested(expected) if expected == request => (
                        GeometryObjectWait::Coalesced(request.key()),
                        Some(RangeTextInputRequest::ObjectPage(request)),
                    ),
                    ObjectDemand::Resident(page) => {
                        object_touches[1] = Some(page);
                        (GeometryObjectWait::Resident(page), None)
                    }
                    ObjectDemand::Coalesced(existing) => {
                        (GeometryObjectWait::Coalesced(existing), None)
                    }
                    _ => return Err(RangeTextInputError::Stale),
                };
                pending_object = Some(PendingGeometryObject {
                    job,
                    request,
                    text_page,
                    wait,
                });
                (None, Some(prepared), effect)
            }
        };

        let release_effect = match (completed_page, completed_object_page) {
            (Some(key), None) => Some(RangeTextInputRequest::ReleasePage(key)),
            (None, Some(key)) => Some(RangeTextInputRequest::ReleaseObjectPage(key)),
            (None, None) => None,
            (Some(_), Some(_)) => return Err(RangeTextInputError::Stale),
        };
        let effects = [release_effect, request_effect];
        let effect_count = effects.iter().flatten().count();
        let destination_capacity = self
            .requests
            .len()
            .checked_add(effect_count)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let requests = VecDeque::with_capacity(destination_capacity);
        let text_allocation = text_admission.as_ref().map_or(Ok((0, 0)), |admission| {
            let charge = admission.page().retained_charge();
            Ok::<_, RangeTextInputError>((
                admission
                    .retained_bytes()
                    .checked_sub(charge.bytes())
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
                admission
                    .retained_items()
                    .checked_sub(charge.items())
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
            ))
        })?;
        let object_allocation = object_admission.as_ref().map_or(Ok((0, 0)), |admission| {
            let charge = admission.page().retained_charge();
            Ok::<_, RangeTextInputError>((
                admission
                    .retained_bytes()
                    .checked_sub(charge.bytes())
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
                admission
                    .retained_items()
                    .checked_sub(
                        charge
                            .objects()
                            .checked_add(1)
                            .ok_or(RangeTextInputError::SurfaceCapacity)?,
                    )
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
            ))
        })?;
        let mut residency_payload = Self::resident_publication_payload_charge(
            self.residency.resident_page_iter(),
            self.object_residency.resident_page_iter(),
        )?;
        if let Some(admission) = text_admission.as_ref() {
            let charge = admission.page().retained_charge();
            residency_payload.bytes = residency_payload
                .bytes
                .checked_add(charge.bytes())
                .ok_or(RangeTextInputError::SurfaceCapacity)?;
            residency_payload.items = residency_payload
                .items
                .checked_add(charge.items())
                .ok_or(RangeTextInputError::SurfaceCapacity)?;
        }
        if let Some(admission) = object_admission.as_ref() {
            let charge = admission.page().retained_charge();
            residency_payload.bytes = residency_payload
                .bytes
                .checked_add(charge.bytes())
                .ok_or(RangeTextInputError::SurfaceCapacity)?;
            residency_payload.items = residency_payload
                .items
                .checked_add(
                    charge
                        .objects()
                        .checked_add(1)
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                )
                .ok_or(RangeTextInputError::SurfaceCapacity)?;
        }
        let demand_bytes = text_demand
            .as_ref()
            .map_or(0, |demand| demand.retained_bytes())
            .checked_add(
                object_demand
                    .as_ref()
                    .map_or(0, |demand| demand.retained_bytes()),
            )
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let demand_items = text_demand
            .as_ref()
            .map_or(0, |demand| demand.retained_items())
            .checked_add(
                object_demand
                    .as_ref()
                    .map_or(0, |demand| demand.retained_items()),
            )
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let prior = self
            .surface
            .as_ref()
            .map_or(Default::default(), |surface| surface.charge());
        let current_request_bytes = self
            .requests
            .capacity()
            .checked_mul(size_of::<RangeTextInputRequest>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let destination_request_bytes = requests
            .capacity()
            .checked_mul(size_of::<RangeTextInputRequest>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let candidate_bytes = [
            size_of::<PreparedNonterminalResponsePublication>(),
            geometry.retained_bytes(),
            text_allocation.0,
            object_allocation.0,
            residency_payload.bytes,
            demand_bytes,
            destination_request_bytes,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let candidate_items = [
            1,
            geometry.retained_items(),
            text_allocation.1,
            object_allocation.1,
            residency_payload.items,
            demand_items,
            requests.capacity(),
            effect_count,
            usize::from(pending_page.is_some()),
            usize::from(pending_object.is_some()),
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let admission_charge = crate::RangeSurfaceCharge {
            bytes: prior
                .bytes
                .checked_add(current_request_bytes)
                .and_then(|bytes| bytes.checked_add(candidate_bytes))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: prior
                .items
                .checked_add(self.requests.capacity())
                .and_then(|items| items.checked_add(candidate_items))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if admission_charge.bytes > self.config.limits.max_surface_bytes
            || admission_charge.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        Ok(PreparedNonterminalResponsePublication {
            geometry,
            text_admission,
            object_admission,
            text_demand,
            object_demand,
            text_touches,
            object_touches,
            pending_page,
            pending_object,
            requests,
            effects,
            completed_page,
            completed_object_page,
            next_id: self.next_id + 1,
        })
    }

    fn prepare_target_publication_from<'a>(
        &self,
        target: &crate::BlockTargetPublication,
        mut state: SurfaceCandidate,
        pages: impl ExactSizeIterator<Item = &'a crate::RangePage> + Clone,
        object_pages: impl ExactSizeIterator<Item = &'a crate::ObjectPage> + Clone,
    ) -> Result<TerminalTargetPreparation, RangeTextInputError> {
        let index = self.geometry.index().ok_or(RangeTextInputError::Stale)?;
        let aggregate = index.aggregate();
        if state.binding != self.config.binding
            || state.job != target.key()
            || state.job.geometry() != self.geometry.key()
        {
            return Err(RangeTextInputError::Stale);
        }
        if self.pending_select_all {
            state.desired.source_selection = Some(index.document_selection());
            state.desired.composition = None;
            state.desired.reveal_caret = true;
            state.desired.inline_object_interaction = self.active_object.map(|_| {
                super::DesiredInlineObjectInteraction::Clear(
                    crate::InlineObjectRealizationLossReason::SelectionChanged,
                )
            });
        }
        let desired = state.desired;
        let required_anchor = if desired.preserve_scroll_anchor {
            Some(desired.scroll.source)
        } else if desired.reveal_caret {
            desired
                .source_selection
                .map(|selection| selection.head.byte_offset)
        } else {
            None
        };
        if let Some(anchor) = required_anchor
            && (anchor < target.predecessor().byte_offset
                || anchor > target.source_end().byte_offset)
        {
            let mut retarget = desired;
            retarget.target_block = if anchor < target.predecessor().byte_offset {
                index
                    .checkpoints()
                    .iter()
                    .rev()
                    .find(|checkpoint| checkpoint.source().byte_offset <= anchor)
                    .map(|checkpoint| checkpoint.block_offset())
                    .ok_or(RangeTextInputError::IncompleteSurface)?
            } else {
                desired.target_block + desired.viewport_extent.max(self.config.layout.line_height)
            };
            return Ok(TerminalTargetPreparation::Retarget(retarget));
        }
        let preserved_scroll_position =
            state
                .restoration
                .map(|seed| seed.scroll.position)
                .or_else(|| {
                    desired
                        .preserve_scroll_anchor
                        .then(|| {
                            self.surface
                                .as_ref()
                                .map(|surface| surface.scroll_position())
                        })
                        .flatten()
                });
        let surface = CoherentRangeSurface::prepare(
            state.binding,
            pages.clone(),
            object_pages.clone(),
            desired,
            state.restoration.map(|seed| (seed.caret, seed.selection)),
            preserved_scroll_position,
            target,
            aggregate.visual_lines(),
            aggregate.content_height(),
            self.config.layout.line_height,
            self.config.layout.wrap_width,
            self.config.placeholder.clone(),
        )?;
        if let Some(seed) = state.restoration
            && (surface.binding() != seed.binding
                || surface.selection().head != seed.caret
                || surface.selection() != seed.selection
                || surface.scroll_source() != seed.scroll.position.byte_offset
                || surface.scroll_intra_anchor() != seed.scroll.intra_anchor)
        {
            return Err(RangeTextInputError::MalformedSeed);
        }
        let (active_result, active_loss, activation) =
            resolve_prepared_active_object(self.active_object, desired, &surface)?;
        let select_all = self.pending_select_all.then_some(surface.selection());
        let resident_payload_charge =
            Self::resident_publication_payload_charge(pages.clone(), object_pages.clone())?;
        let pages = Vec::with_capacity(pages.len());
        let object_pages = Vec::with_capacity(object_pages.len());
        let page_slot_bytes = pages
            .capacity()
            .checked_mul(std::mem::size_of::<crate::RangePage>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let object_page_slot_bytes = object_pages
            .capacity()
            .checked_mul(std::mem::size_of::<crate::ObjectPage>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let prepared_allocation_charge = crate::RangeSurfaceCharge {
            bytes: surface
                .candidate_charge()
                .bytes
                .checked_add(page_slot_bytes)
                .and_then(|bytes| bytes.checked_add(object_page_slot_bytes))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: surface
                .candidate_charge()
                .items
                .checked_add(pages.capacity())
                .and_then(|items| items.checked_add(object_pages.capacity()))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        Ok(TerminalTargetPreparation::Publication(
            PreparedTargetPublication {
                state,
                surface,
                resident_payload_charge,
                prepared_allocation_charge,
                pages,
                object_pages,
                select_all,
                active_loss,
                active_result,
                activation,
            },
        ))
    }

    fn resident_publication_payload_charge<'a>(
        mut pages: impl Iterator<Item = &'a crate::RangePage>,
        mut object_pages: impl Iterator<Item = &'a crate::ObjectPage>,
    ) -> Result<crate::RangeSurfaceCharge, RangeTextInputError> {
        let text = pages.try_fold(crate::RangeSurfaceCharge::default(), |charge, page| {
            Ok::<_, RangeTextInputError>(crate::RangeSurfaceCharge {
                bytes: charge
                    .bytes
                    .checked_add(page.retained_charge().bytes())
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
                items: charge
                    .items
                    .checked_add(page.retained_charge().items())
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
            })
        })?;
        object_pages.try_fold(text, |charge, page| {
            Ok(crate::RangeSurfaceCharge {
                bytes: charge
                    .bytes
                    .checked_add(page.retained_charge().bytes())
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
                items: charge
                    .items
                    .checked_add(
                        page.objects()
                            .len()
                            .checked_add(1)
                            .ok_or(RangeTextInputError::SurfaceCapacity)?,
                    )
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
            })
        })
    }

    fn prepare_terminal_response_publication(
        &self,
        geometry: crate::range_geometry::PreparedTargetResponse,
        text_admission: Option<crate::residency::PreparedRangePageAdmission>,
        object_admission: Option<crate::object_residency::PreparedObjectPageAdmission>,
        text_touch: Option<PageId>,
        object_touch: Option<ObjectPageId>,
        completed_page: Option<PageRequestKey>,
        completed_object_page: Option<ObjectRequestKey>,
    ) -> Result<PreparedTerminalResponsePublication, RangeTextInputError> {
        if geometry.progress() != ExactGeometryProgress::TargetComplete
            || geometry
                .release()
                .pages
                .iter()
                .any(|key| Some(*key) != completed_page)
            || geometry
                .release()
                .object_pages
                .iter()
                .any(|key| Some(*key) != completed_object_page)
        {
            return Err(RangeTextInputError::Stale);
        }
        let target = geometry
            .terminal_target()
            .ok_or(RangeTextInputError::Stale)?;
        let state = *self
            .surface_candidate
            .as_ref()
            .filter(|state| state.job == target.key())
            .ok_or(RangeTextInputError::Stale)?;
        let preparation = match (&text_admission, &object_admission) {
            (Some(text), None) => self.prepare_target_publication_from(
                target,
                state,
                text.projected_resident_pages(&self.residency),
                self.object_residency
                    .resident_pages_after_touch(object_touch),
            )?,
            (None, Some(objects)) => self.prepare_target_publication_from(
                target,
                state,
                self.residency.resident_pages_after_touch(text_touch),
                objects.projected_resident_pages(&self.object_residency),
            )?,
            (None, None) => self.prepare_target_publication_from(
                target,
                state,
                self.residency.resident_pages_after_touch(text_touch),
                self.object_residency
                    .resident_pages_after_touch(object_touch),
            )?,
            (Some(_), Some(_)) => return Err(RangeTextInputError::Stale),
        };
        let TerminalTargetPreparation::Publication(publication) = preparation else {
            return Err(RangeTextInputError::IncompleteSurface);
        };
        let release_request = match (completed_page, completed_object_page) {
            (Some(key), None) if text_admission.is_some() => {
                Some(RangeTextInputRequest::ReleasePage(key))
            }
            (None, Some(key)) if object_admission.is_some() => {
                Some(RangeTextInputRequest::ReleaseObjectPage(key))
            }
            (Some(_), None) | (None, Some(_)) => None,
            _ => return Err(RangeTextInputError::Stale),
        };
        let destination_capacity = self
            .requests
            .len()
            .checked_add(usize::from(release_request.is_some()))
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let requests = VecDeque::with_capacity(destination_capacity);
        let text_allocation = text_admission.as_ref().map_or(Ok((0, 0)), |admission| {
            let charge = admission.page().retained_charge();
            Ok::<_, RangeTextInputError>((
                admission
                    .retained_bytes()
                    .checked_sub(charge.bytes())
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
                admission
                    .retained_items()
                    .checked_sub(charge.items())
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
            ))
        })?;
        let object_allocation = object_admission.as_ref().map_or(Ok((0, 0)), |admission| {
            let charge = admission.page().retained_charge();
            Ok::<_, RangeTextInputError>((
                admission
                    .retained_bytes()
                    .checked_sub(charge.bytes())
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
                admission
                    .retained_items()
                    .checked_sub(
                        charge
                            .objects()
                            .checked_add(1)
                            .ok_or(RangeTextInputError::SurfaceCapacity)?,
                    )
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
            ))
        })?;
        let mut residency_payload = Self::resident_publication_payload_charge(
            self.residency.resident_page_iter(),
            self.object_residency.resident_page_iter(),
        )?;
        if let Some(admission) = text_admission.as_ref() {
            let charge = admission.page().retained_charge();
            residency_payload.bytes = residency_payload
                .bytes
                .checked_add(charge.bytes())
                .ok_or(RangeTextInputError::SurfaceCapacity)?;
            residency_payload.items = residency_payload
                .items
                .checked_add(charge.items())
                .ok_or(RangeTextInputError::SurfaceCapacity)?;
        }
        if let Some(admission) = object_admission.as_ref() {
            let charge = admission.page().retained_charge();
            residency_payload.bytes = residency_payload
                .bytes
                .checked_add(charge.bytes())
                .ok_or(RangeTextInputError::SurfaceCapacity)?;
            residency_payload.items = residency_payload
                .items
                .checked_add(
                    charge
                        .objects()
                        .checked_add(1)
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                )
                .ok_or(RangeTextInputError::SurfaceCapacity)?;
        }
        let prior = self
            .surface
            .as_ref()
            .map_or(crate::RangeSurfaceCharge::default(), |surface| {
                surface.charge()
            });
        let current_request_bytes = self
            .requests
            .capacity()
            .checked_mul(size_of::<RangeTextInputRequest>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let destination_request_bytes = requests
            .capacity()
            .checked_mul(size_of::<RangeTextInputRequest>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let candidate_bytes = [
            size_of::<PreparedTerminalResponsePublication>(),
            geometry.retained_bytes(),
            text_allocation.0,
            object_allocation.0,
            residency_payload.bytes,
            publication.prepared_allocation_charge().bytes,
            destination_request_bytes,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let candidate_items = [
            1,
            geometry.retained_items(),
            text_allocation.1,
            object_allocation.1,
            residency_payload.items,
            publication.prepared_allocation_charge().items,
            requests.capacity(),
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let admission_charge = crate::RangeSurfaceCharge {
            bytes: prior
                .bytes
                .checked_add(current_request_bytes)
                .and_then(|bytes| bytes.checked_add(candidate_bytes))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: prior
                .items
                .checked_add(self.requests.capacity())
                .and_then(|items| items.checked_add(candidate_items))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if admission_charge.bytes > self.config.limits.max_surface_bytes
            || admission_charge.items > self.config.limits.max_surface_items
            || publication.final_charge().bytes > self.config.limits.max_surface_bytes
            || publication.final_charge().items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        Ok(PreparedTerminalResponsePublication {
            geometry,
            text_admission,
            object_admission,
            text_touch,
            object_touch,
            publication,
            requests,
            release_request,
            completed_page,
            completed_object_page,
            admission_charge,
        })
    }

    pub(super) fn commit_prepared_target_publication(
        &mut self,
        prepared: PreparedTargetPublication,
        admission: crate::RangeSurfaceCharge,
    ) {
        let PreparedTargetPublication {
            state,
            surface,
            pages,
            object_pages,
            select_all,
            active_result,
            ..
        } = prepared;
        let target = self
            .geometry
            .take_target()
            .expect("terminal geometry target was prepared");
        let pages = self.residency.take_resident_pages_into(pages);
        let object_pages = self.object_residency.take_resident_pages_into(object_pages);
        let surface = CoherentRangeSurface::commit_prepared(surface, pages, object_pages, target);
        self.last_surface_admission = Some(admission);
        if let Some(active_result) = active_result {
            self.active_object = active_result;
        }
        self.surface_candidate = None;
        self.surface = Some(surface);
        if let Some(seed) = state.restoration {
            self.restoration_seed = None;
            self.published_restoration = Some(seed);
        }
        let surface = self.surface.as_ref().expect("terminal surface committed");
        self.desired.source_selection = Some(surface.selection());
        self.desired.scroll = RangeScrollAnchor {
            source: surface.scroll_source(),
            intra_anchor: surface.scroll_intra_anchor(),
        };
        self.desired.target_block = surface.scroll_block();
        self.desired.preserve_scroll_anchor = false;
        self.desired.reveal_caret = false;
        self.desired.inline_object_interaction = None;
        if let Some(selection) = select_all {
            self.pending_select_all = false;
            self.desired.source_selection = Some(selection);
            self.desired.reveal_caret = false;
        }
    }

    fn commit_terminal_response_publication(
        &mut self,
        candidate: PreparedTerminalResponsePublication,
        cx: &mut Context<Self>,
    ) {
        let PreparedTerminalResponsePublication {
            geometry,
            text_admission,
            object_admission,
            text_touch,
            object_touch,
            publication,
            requests,
            release_request,
            completed_page,
            completed_object_page,
            admission_charge,
        } = candidate;
        let active_loss = publication.active_loss();
        let activation = publication.activation();
        let admission = self.geometry.commit_prepared_target_response(geometry);
        debug_assert_eq!(admission.progress(), ExactGeometryProgress::TargetComplete);
        if let Some(admission) = text_admission {
            self.residency.commit_prepared_admit(admission);
        }
        if let Some(admission) = object_admission {
            self.object_residency.commit_prepared_admit(admission);
        }
        if let Some(page) = text_touch {
            self.residency.commit_page_touch(page);
        }
        if let Some(page) = object_touch {
            self.object_residency.commit_page_touch(page);
        }
        if let Some(key) = completed_page {
            self.dispatched_pages.remove(&key);
        }
        if let Some(key) = completed_object_page {
            self.dispatched_object_pages.remove(&key);
        }
        self.active_geometry = None;
        self.pending_geometry_page = None;
        self.pending_geometry_object = None;
        let mut prior_requests = std::mem::replace(&mut self.requests, requests);
        while let Some(request) = prior_requests.pop_front() {
            self.requests.push_back(request);
        }
        if let Some(release_request) = release_request {
            self.requests.push_back(release_request);
        }
        self.commit_prepared_target_publication(publication, admission_charge);
        if let Some(loss) = active_loss {
            cx.emit(RangeTextInputEvent::InlineObjectRealizationLost(loss));
        }
        if let Some(activation) = activation {
            cx.emit(RangeTextInputEvent::InlineObjectActivated(activation));
        }
        cx.notify();
    }

    fn commit_nonterminal_response_publication(
        &mut self,
        candidate: PreparedNonterminalResponsePublication,
        cx: &mut Context<Self>,
    ) {
        let PreparedNonterminalResponsePublication {
            geometry,
            text_admission,
            object_admission,
            text_demand,
            object_demand,
            text_touches,
            object_touches,
            pending_page,
            pending_object,
            requests,
            effects,
            completed_page,
            completed_object_page,
            next_id,
        } = candidate;
        let admission = self.geometry.commit_prepared_target_response(geometry);
        debug_assert_ne!(admission.progress(), ExactGeometryProgress::TargetComplete);
        if let Some(admission) = text_admission {
            self.residency.commit_prepared_admit(admission);
        }
        if let Some(admission) = object_admission {
            self.object_residency.commit_prepared_admit(admission);
        }
        if let Some(demand) = text_demand {
            self.residency.commit_prepared_demand(demand);
        }
        if let Some(demand) = object_demand {
            self.object_residency.commit_prepared_demand(demand);
        }
        for page in text_touches.into_iter().flatten() {
            self.residency.commit_page_touch(page);
        }
        for page in object_touches.into_iter().flatten() {
            self.object_residency.commit_page_touch(page);
        }
        if let Some(key) = completed_page {
            self.dispatched_pages.remove(&key);
        }
        if let Some(key) = completed_object_page {
            self.dispatched_object_pages.remove(&key);
        }
        self.pending_geometry_page = pending_page;
        self.pending_geometry_object = pending_object;
        let mut prior_requests = std::mem::replace(&mut self.requests, requests);
        while let Some(request) = prior_requests.pop_front() {
            self.requests.push_back(request);
        }
        for effect in effects.into_iter().flatten() {
            self.requests.push_back(effect);
        }
        self.next_id = next_id;
        cx.notify();
    }

    pub(super) fn install_prepared_geometry_page(
        &mut self,
        job: crate::GeometryJobKey,
        request: PageRequest,
        demand: PageDemand,
    ) {
        match demand {
            PageDemand::Requested(expected) => {
                debug_assert_eq!(expected.key(), request.key());
            }
            PageDemand::ResidentAdjacent(page) => {
                self.pending_geometry_page = Some(PendingGeometryPage {
                    job,
                    request,
                    wait: GeometryPageWait::Resident(page),
                });
            }
            PageDemand::Coalesced(existing) => {
                self.pending_geometry_page = Some(PendingGeometryPage {
                    job,
                    request,
                    wait: GeometryPageWait::Coalesced(existing),
                });
            }
            PageDemand::ResidentValidation { .. } => {
                unreachable!("geometry uses adjacent page demand")
            }
        }
    }

    pub(super) fn start_index(&mut self) -> Result<(), RangeTextInputError> {
        let candidate = self.prepare_index_transition()?;
        match self.commit_widget_transition(candidate, None) {
            ExactGeometryProgress::Scanning => Ok(()),
            _ => Err(RangeTextInputError::Stale),
        }
    }

    pub(super) fn start_target(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let desired = self.desired;
        if self.reject_restoration_task(cx)? {
            self.desired = desired;
            return self.start_index();
        }
        self.start_target_for(Some(cx))
    }

    fn start_target_for(
        &mut self,
        cx: Option<&mut Context<Self>>,
    ) -> Result<(), RangeTextInputError> {
        let candidate = self.prepare_target_transition(self.desired, None)?;
        match self.commit_widget_transition(candidate, cx) {
            ExactGeometryProgress::TargetComplete => Ok(()),
            ExactGeometryProgress::Scanning | ExactGeometryProgress::PendingIndex => Ok(()),
            _ => Err(RangeTextInputError::Stale),
        }
    }

    fn start_or_resume_target(
        &mut self,
        cx: Option<&mut Context<Self>>,
    ) -> Result<(), RangeTextInputError> {
        let restoration = self
            .surface_candidate
            .as_ref()
            .and_then(|candidate| candidate.restoration)
            .or(self.restoration_seed);
        let mut desired = self.desired;
        if self.pending_select_all {
            let index = self.geometry.index().ok_or(RangeTextInputError::Stale)?;
            desired.source_selection = Some(index.document_selection());
            desired.composition = None;
            desired.target_block = index.aggregate().content_height();
            desired.reveal_caret = true;
            desired.inline_object_interaction = self.active_object.map(|_| {
                super::DesiredInlineObjectInteraction::Clear(
                    crate::InlineObjectRealizationLossReason::SelectionChanged,
                )
            });
        }
        let candidate = self.prepare_target_transition(desired, restoration)?;
        match self.commit_widget_transition(candidate, cx) {
            ExactGeometryProgress::TargetComplete
            | ExactGeometryProgress::Scanning
            | ExactGeometryProgress::PendingIndex => Ok(()),
            _ => Err(RangeTextInputError::Stale),
        }
    }

    fn request_geometry_page(
        &mut self,
        job: crate::GeometryJobKey,
    ) -> Result<(), RangeTextInputError> {
        let id = PageRequestId::new(self.next_id());
        let request = self.geometry.request_page(job, id)?;
        let demand = self
            .residency
            .demand(id, request.key().purpose(), request.key().demand())
            .map_err(|_| RangeTextInputError::Busy);
        let demand = match demand {
            Ok(demand) => demand,
            Err(error) => {
                self.abort_geometry_demand(job, None)?;
                return Err(error);
            }
        };
        match demand {
            PageDemand::Requested(expected) if expected.key() == request.key() => {
                self.requests
                    .push_back(RangeTextInputRequest::Page(request));
                Ok(())
            }
            PageDemand::ResidentAdjacent(page) => {
                self.pending_geometry_page = Some(PendingGeometryPage {
                    job,
                    request,
                    wait: GeometryPageWait::Resident(page),
                });
                Ok(())
            }
            PageDemand::Coalesced(existing) => {
                self.pending_geometry_page = Some(PendingGeometryPage {
                    job,
                    request,
                    wait: GeometryPageWait::Coalesced(existing),
                });
                Ok(())
            }
            _ => {
                let _ = self.residency.cancel(request.key());
                self.abort_geometry_demand(job, None)?;
                Err(RangeTextInputError::Stale)
            }
        }
    }

    fn request_geometry_object(
        &mut self,
        job: crate::GeometryJobKey,
    ) -> Result<(), RangeTextInputError> {
        let text_page = self
            .geometry
            .active_text_page(job)
            .ok_or(RangeTextInputError::Stale)?;
        let id = ObjectRequestId::new(self.next_id());
        let limits = self.config.object_residency_limits;
        let request = self.geometry.request_object_page(
            job,
            id,
            limits.max_resident_objects(),
            limits.max_resident_bytes(),
        )?;
        let demand = self
            .object_residency
            .demand(id, request.key().purpose(), request.key().demand())
            .map_err(|_| RangeTextInputError::Busy);
        let demand = match demand {
            Ok(demand) => demand,
            Err(error) => {
                self.abort_geometry_demand(job, None)?;
                return Err(error);
            }
        };
        match demand {
            ObjectDemand::Requested(expected) if expected.key() == request.key() => {
                self.pending_geometry_object = Some(PendingGeometryObject {
                    job,
                    request,
                    text_page,
                    wait: GeometryObjectWait::Coalesced(request.key()),
                });
                self.requests
                    .push_back(RangeTextInputRequest::ObjectPage(request));
                Ok(())
            }
            ObjectDemand::Resident(page) => self.reissue_geometry_object_from_resident(
                PendingGeometryObject {
                    job,
                    request,
                    text_page,
                    wait: GeometryObjectWait::Resident(page),
                },
                page,
                None,
            ),
            ObjectDemand::Coalesced(existing) => {
                self.pending_geometry_object = Some(PendingGeometryObject {
                    job,
                    request,
                    text_page,
                    wait: GeometryObjectWait::Coalesced(existing),
                });
                Ok(())
            }
            _ => {
                let _ = self.object_residency.cancel(request.key());
                self.abort_geometry_demand(job, None)?;
                Err(RangeTextInputError::Stale)
            }
        }
    }

    fn abort_geometry_demand(
        &mut self,
        job: crate::GeometryJobKey,
        cx: Option<&mut Context<Self>>,
    ) -> Result<(), RangeTextInputError> {
        let release = self.geometry.cancel(job)?;
        self.release_geometry(&release, None, None, cx);
        if self.active_geometry == Some(job) {
            self.active_geometry = None;
        }
        Ok(())
    }

    pub(super) fn service_geometry_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let result = self
            .service_geometry_object(window, cx)
            .and_then(|()| self.service_geometry_page_inner(window, cx));
        if result.is_err() {
            self.reject_restoration_geometry(cx)?;
        }
        result
    }

    pub(super) fn service_geometry_until_external_boundary(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let limit = self
            .config
            .residency_limits
            .max_resident_pages()
            .saturating_add(self.config.object_residency_limits.max_resident_objects())
            .saturating_add(2);
        for _ in 0..limit {
            let before = self.geometry_service_marker();
            self.service_geometry_page(window, cx)?;
            if self.geometry_service_marker() == before {
                return Ok(());
            }
        }
        Err(RangeTextInputError::Busy)
    }

    fn geometry_service_marker(
        &self,
    ) -> (
        Option<crate::GeometryJobKey>,
        Option<(crate::GeometryJobKey, PageRequestKey, GeometryPageWait)>,
        Option<(crate::GeometryJobKey, ObjectRequestKey, GeometryObjectWait)>,
        usize,
    ) {
        (
            self.active_geometry,
            self.pending_geometry_page
                .as_ref()
                .map(|pending| (pending.job, pending.request.key(), pending.wait)),
            self.pending_geometry_object
                .as_ref()
                .map(|pending| (pending.job, pending.request.key(), pending.wait)),
            self.requests.len(),
        )
    }

    fn service_resident_target_object(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Result<(), RangeTextInputError>> {
        let pending = self.pending_geometry_object.as_ref()?;
        let GeometryObjectWait::Resident(object_page_id) = pending.wait else {
            return None;
        };
        if pending.request.key().purpose() != crate::ObjectPurpose::GeometryTarget {
            return None;
        }
        let job = pending.job;
        let key = pending.request.key();
        let text_page_id = pending.text_page;
        if self.active_geometry != Some(job) {
            return Some(Err(RangeTextInputError::Stale));
        }
        let geometry = {
            let Some(text_page) = self.residency.peek_page_by_id(text_page_id) else {
                return Some(Err(RangeTextInputError::Stale));
            };
            let Some(object_page) = self.object_residency.peek_page_by_id(object_page_id) else {
                return Some(Err(RangeTextInputError::Stale));
            };
            self.geometry.prepare_target_object_page(
                job,
                text_page,
                object_page,
                window.text_system(),
                match self.target_response_successor() {
                    Ok(successor) => successor,
                    Err(error) => return Some(Err(error)),
                },
            )
        };
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                return Some(Err(RangeTextInputError::Geometry(failure.error().clone())));
            }
        };
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let candidate = match self.prepare_terminal_response_publication(
                geometry,
                None,
                None,
                Some(text_page_id),
                Some(object_page_id),
                None,
                Some(key),
            ) {
                Ok(candidate) => candidate,
                Err(error) => return Some(Err(error)),
            };
            self.commit_terminal_response_publication(candidate, cx);
            return Some(Ok(()));
        }
        let candidate = match self.prepare_nonterminal_response_publication(
            geometry,
            None,
            None,
            Some(text_page_id),
            Some(object_page_id),
            None,
            Some(key),
            None,
            None,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return Some(Err(error)),
        };
        self.commit_nonterminal_response_publication(candidate, cx);
        Some(Ok(()))
    }

    fn service_geometry_object(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if let Some(result) = self.service_resident_target_object(window, cx) {
            return result;
        }
        let Some(mut pending) = self.pending_geometry_object.take() else {
            return Ok(());
        };
        if self.active_geometry != Some(pending.job) {
            return Err(RangeTextInputError::Stale);
        }
        if let GeometryObjectWait::Coalesced(existing) = pending.wait {
            if self
                .object_residency
                .pending_requests()
                .any(|request| request == existing)
            {
                self.pending_geometry_object = Some(pending);
                return Ok(());
            }
            let demand = match self.object_residency.demand(
                pending.request.key().id(),
                pending.request.key().purpose(),
                pending.request.key().demand(),
            ) {
                Ok(demand) => demand,
                Err(_) => {
                    self.abort_geometry_demand(pending.job, Some(cx))?;
                    return Err(RangeTextInputError::Busy);
                }
            };
            pending.wait = match demand {
                ObjectDemand::Resident(page) => {
                    return self.reissue_geometry_object_from_resident(pending, page, Some(cx));
                }
                ObjectDemand::Coalesced(request) => GeometryObjectWait::Coalesced(request),
                ObjectDemand::Requested(request) if request.key() == pending.request.key() => {
                    self.requests
                        .push_back(RangeTextInputRequest::ObjectPage(request));
                    self.pending_geometry_object = Some(pending);
                    cx.notify();
                    return Ok(());
                }
                _ => {
                    self.abort_geometry_demand(pending.job, Some(cx))?;
                    return Err(RangeTextInputError::Stale);
                }
            };
        }
        let GeometryObjectWait::Resident(object_page_id) = pending.wait else {
            self.pending_geometry_object = Some(pending);
            return Ok(());
        };
        let admission = {
            let text_residency = &mut self.residency;
            let object_residency = &mut self.object_residency;
            let geometry = &mut self.geometry;
            let text_page = text_residency
                .page_by_id(pending.text_page)
                .ok_or(RangeTextInputError::Stale)?;
            let object_page = object_residency
                .page_by_id(object_page_id)
                .ok_or(RangeTextInputError::Stale)?;
            geometry.admit_object_page(pending.job, text_page, object_page, window.text_system())
        };
        let admission = match admission {
            Ok(admission) => admission,
            Err(failure) => {
                self.release_geometry(
                    failure.release(),
                    None,
                    Some(pending.request.key()),
                    Some(cx),
                );
                self.active_geometry = None;
                return Err(RangeTextInputError::Geometry(failure.error().clone()));
            }
        };
        self.release_geometry(
            admission.release(),
            None,
            Some(pending.request.key()),
            Some(cx),
        );
        self.advance_geometry(pending.job, admission.progress(), cx)
    }

    fn reissue_geometry_object_from_resident(
        &mut self,
        mut pending: PendingGeometryObject,
        page: ObjectPageId,
        cx: Option<&mut Context<Self>>,
    ) -> Result<(), RangeTextInputError> {
        let resident_key = self
            .object_residency
            .page_by_id(page)
            .ok_or(RangeTextInputError::Stale)?
            .key();
        if !self.object_residency.evict(page) {
            return Err(RangeTextInputError::Stale);
        }
        self.requests
            .push_back(RangeTextInputRequest::ReleaseObjectPage(resident_key));
        let key = pending.request.key();
        let demand = match self
            .object_residency
            .demand(key.id(), key.purpose(), key.demand())
        {
            Ok(demand) => demand,
            Err(_) => {
                self.abort_geometry_demand(pending.job, cx)?;
                return Err(RangeTextInputError::Busy);
            }
        };
        pending.wait = match demand {
            ObjectDemand::Requested(request) if request.key() == key => {
                self.requests
                    .push_back(RangeTextInputRequest::ObjectPage(request));
                GeometryObjectWait::Coalesced(key)
            }
            ObjectDemand::Coalesced(existing) => GeometryObjectWait::Coalesced(existing),
            _ => {
                self.abort_geometry_demand(pending.job, cx)?;
                return Err(RangeTextInputError::Stale);
            }
        };
        self.pending_geometry_object = Some(pending);
        if let Some(cx) = cx {
            cx.notify();
        }
        Ok(())
    }

    fn service_resident_target_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Result<(), RangeTextInputError>> {
        let pending = self.pending_geometry_page.as_ref()?;
        let GeometryPageWait::Resident(page_id) = pending.wait else {
            return None;
        };
        if pending.request.key().purpose() != crate::PagePurpose::GeometryTarget {
            return None;
        }
        let job = pending.job;
        let key = pending.request.key();
        if self.active_geometry != Some(job) {
            return Some(Err(RangeTextInputError::Stale));
        }
        let geometry = {
            let Some(page) = self.residency.peek_page_by_id(page_id) else {
                return Some(Err(RangeTextInputError::Stale));
            };
            self.geometry.prepare_target_resident_page(
                job,
                page,
                window.text_system(),
                match self.target_response_successor() {
                    Ok(successor) => successor,
                    Err(error) => return Some(Err(error)),
                },
            )
        };
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                return Some(Err(RangeTextInputError::Geometry(failure.error().clone())));
            }
        };
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let candidate = match self.prepare_terminal_response_publication(
                geometry,
                None,
                None,
                Some(page_id),
                None,
                Some(key),
                None,
            ) {
                Ok(candidate) => candidate,
                Err(error) => return Some(Err(error)),
            };
            self.commit_terminal_response_publication(candidate, cx);
            return Some(Ok(()));
        }
        let candidate = match self.prepare_nonterminal_response_publication(
            geometry,
            None,
            None,
            Some(page_id),
            None,
            Some(key),
            None,
            None,
            None,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return Some(Err(error)),
        };
        self.commit_nonterminal_response_publication(candidate, cx);
        Some(Ok(()))
    }

    fn service_geometry_page_inner(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if let Some(result) = self.service_resident_target_page(window, cx) {
            return result;
        }
        let Some(mut pending) = self.pending_geometry_page.take() else {
            return Ok(());
        };
        if self.active_geometry != Some(pending.job) {
            return Err(RangeTextInputError::Stale);
        }
        if let GeometryPageWait::Coalesced(existing) = pending.wait {
            if self
                .residency
                .pending_requests()
                .any(|request| request == existing)
            {
                self.pending_geometry_page = Some(pending);
                return Ok(());
            }
            let demand = match self.residency.demand(
                pending.request.key().id(),
                pending.request.key().purpose(),
                pending.request.key().demand(),
            ) {
                Ok(demand) => demand,
                Err(_) => {
                    self.pending_geometry_page = Some(pending);
                    return Err(RangeTextInputError::Busy);
                }
            };
            pending.wait = match demand {
                PageDemand::ResidentAdjacent(page) => GeometryPageWait::Resident(page),
                PageDemand::Coalesced(request) => GeometryPageWait::Coalesced(request),
                PageDemand::Requested(request) if request.key() == pending.request.key() => {
                    self.requests
                        .push_back(RangeTextInputRequest::Page(request));
                    cx.notify();
                    return Ok(());
                }
                _ => return Err(RangeTextInputError::Stale),
            };
            if pending.request.key().purpose() == crate::PagePurpose::GeometryTarget
                && matches!(pending.wait, GeometryPageWait::Resident(_))
            {
                self.pending_geometry_page = Some(pending);
                return self
                    .service_resident_target_page(window, cx)
                    .expect("resident target page was restored");
            }
        }
        let GeometryPageWait::Resident(page_id) = pending.wait else {
            self.pending_geometry_page = Some(pending);
            return Ok(());
        };
        let admission = {
            let Some(page) = self.residency.page_by_id(page_id) else {
                if let Ok(release) = self.geometry.cancel(pending.job) {
                    self.release_geometry(&release, Some(pending.request.key()), None, Some(cx));
                }
                self.active_geometry = None;
                return Err(RangeTextInputError::Stale);
            };
            self.geometry
                .admit_resident_page(pending.job, page, window.text_system())
        };
        let admission = match admission {
            Ok(admission) => admission,
            Err(failure) => {
                let terminal = failure.release().jobs.contains(&pending.job);
                self.release_geometry(
                    failure.release(),
                    Some(pending.request.key()),
                    None,
                    Some(cx),
                );
                if terminal {
                    self.active_geometry = None;
                } else {
                    if let Ok(release) = self.geometry.cancel(pending.job) {
                        self.release_geometry(
                            &release,
                            Some(pending.request.key()),
                            None,
                            Some(cx),
                        );
                    }
                    self.active_geometry = None;
                }
                return Err(RangeTextInputError::Geometry(failure.error().clone()));
            }
        };
        self.release_geometry(
            admission.release(),
            Some(pending.request.key()),
            None,
            Some(cx),
        );
        self.advance_geometry(pending.job, admission.progress(), cx)
    }

    pub(super) fn geometry_waits_on(&self, key: PageRequestKey) -> bool {
        self.pending_geometry_page
            .as_ref()
            .is_some_and(|pending| matches!(pending.wait, GeometryPageWait::Coalesced(existing) if existing == key))
    }

    pub(super) fn deliver_geometry_object_page(
        &mut self,
        page: crate::ObjectPage,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        let page_id = page.id();
        let pending = self
            .pending_geometry_object
            .as_ref()
            .ok_or(RangeTextInputError::Stale)?;
        if pending.request.key() != key || self.active_geometry != Some(pending.job) {
            return Err(RangeTextInputError::Stale);
        }
        let proofs = match self
            .residency
            .prove_object_page_anchors(self.config.binding, &page)
        {
            Ok(proofs) => proofs,
            Err(_) => {
                return Err(RangeTextInputError::Geometry(
                    crate::ExactGeometryError::SourceContract,
                ));
            }
        };
        let admission = self
            .object_residency
            .prepare_admit(page, proofs)
            .map_err(|_| RangeTextInputError::Stale)?;
        self.object_residency.commit_prepared_admit(admission);
        let pending = self
            .pending_geometry_object
            .as_mut()
            .ok_or(RangeTextInputError::Stale)?;
        pending.wait = GeometryObjectWait::Resident(page_id);
        Ok(())
    }

    pub(super) fn fail_geometry_object_page(
        &mut self,
        key: ObjectRequestKey,
        failure: crate::ObjectPageFailure,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let pending = self
            .pending_geometry_object
            .take()
            .ok_or(RangeTextInputError::Stale)?;
        if pending.request.key() != key || self.active_geometry != Some(pending.job) {
            self.pending_geometry_object = Some(pending);
            return Err(RangeTextInputError::Stale);
        }
        let _ = self.object_residency.settle(key, failure);
        let release = self.geometry.fail_object_page(pending.job, key)?;
        self.release_geometry(&release, None, Some(key), Some(cx));
        self.active_geometry = None;
        Err(RangeTextInputError::Stale)
    }

    pub(super) fn deliver_geometry_page(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let result = self.deliver_geometry_page_inner(page, window, cx);
        if result.is_err() {
            self.reject_restoration_geometry(cx)?;
        }
        result
    }

    pub(super) fn deliver_geometry_target_page(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        let job = self.active_geometry.ok_or(RangeTextInputError::Stale)?;
        if key.purpose() != crate::PagePurpose::GeometryTarget
            || !self.dispatched_pages.contains(&key)
        {
            return Err(RangeTextInputError::Stale);
        }
        let geometry = self
            .geometry
            .prepare_target_page(
                job,
                &page,
                window.text_system(),
                self.target_response_successor()?,
            )
            .map_err(|failure| RangeTextInputError::Geometry(failure.error().clone()))?;
        let text_admission = self
            .residency
            .prepare_admit(page)
            .map_err(|_| RangeTextInputError::Stale)?;
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let candidate = self.prepare_terminal_response_publication(
                geometry,
                Some(text_admission),
                None,
                None,
                None,
                Some(key),
                None,
            )?;
            self.commit_terminal_response_publication(candidate, cx);
            return Ok(());
        }
        let candidate = self.prepare_nonterminal_response_publication(
            geometry,
            Some(text_admission),
            None,
            None,
            None,
            Some(key),
            None,
            Some(key),
            None,
        )?;
        self.commit_nonterminal_response_publication(candidate, cx);
        self.service_geometry_until_external_boundary(window, cx)
    }

    pub(super) fn deliver_geometry_target_object_page(
        &mut self,
        page: crate::ObjectPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        let pending = self
            .pending_geometry_object
            .as_ref()
            .ok_or(RangeTextInputError::Stale)?;
        if key.purpose() != crate::ObjectPurpose::GeometryTarget
            || pending.request.key() != key
            || self.active_geometry != Some(pending.job)
            || !self.dispatched_object_pages.contains(&key)
        {
            return Err(RangeTextInputError::Stale);
        }
        let job = pending.job;
        let text_page_id = pending.text_page;
        let proofs = match self
            .residency
            .prove_object_page_anchors(self.config.binding, &page)
        {
            Ok(proofs) => proofs,
            Err(_) => {
                return Err(RangeTextInputError::Geometry(
                    crate::ExactGeometryError::SourceContract,
                ));
            }
        };
        let object_admission = self
            .object_residency
            .prepare_admit(page, proofs)
            .map_err(|_| RangeTextInputError::Stale)?;
        let geometry = {
            let text_page = self
                .residency
                .peek_page_by_id(text_page_id)
                .ok_or(RangeTextInputError::Stale)?;
            self.geometry.prepare_target_object_page(
                job,
                text_page,
                object_admission.page(),
                window.text_system(),
                self.target_response_successor()?,
            )
        }
        .map_err(|failure| RangeTextInputError::Geometry(failure.error().clone()))?;
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let candidate = self.prepare_terminal_response_publication(
                geometry,
                None,
                Some(object_admission),
                Some(text_page_id),
                None,
                None,
                Some(key),
            )?;
            self.commit_terminal_response_publication(candidate, cx);
            return Ok(());
        }
        let candidate = self.prepare_nonterminal_response_publication(
            geometry,
            None,
            Some(object_admission),
            Some(text_page_id),
            None,
            None,
            Some(key),
            None,
            Some(key),
        )?;
        self.commit_nonterminal_response_publication(candidate, cx);
        self.service_geometry_until_external_boundary(window, cx)
    }

    fn deliver_geometry_page_inner(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let job = self.active_geometry.ok_or(RangeTextInputError::Stale)?;
        let admission = match self.geometry.admit_page(job, &page, window.text_system()) {
            Ok(admission) => admission,
            Err(failure) => {
                let _ = self.residency.settle(page.key(), PageFailure::Malformed);
                let terminal = failure.release().jobs.contains(&job);
                self.release_geometry(failure.release(), Some(page.key()), None, Some(cx));
                if terminal {
                    self.active_geometry = None;
                }
                return Err(RangeTextInputError::Geometry(failure.error().clone()));
            }
        };
        let consumed = page.key();
        self.residency
            .admit(page)
            .map_err(|_| RangeTextInputError::Stale)?;
        self.release_geometry(admission.release(), Some(consumed), None, Some(cx));
        self.advance_geometry(job, admission.progress(), cx)
    }

    fn advance_geometry(
        &mut self,
        job: crate::GeometryJobKey,
        progress: ExactGeometryProgress,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        match progress {
            ExactGeometryProgress::Scanning => self.request_geometry_page(job),
            ExactGeometryProgress::NeedObjects => self.request_geometry_object(job),
            ExactGeometryProgress::IndexComplete => {
                self.active_geometry = None;
                drop(self.object_residency.take_resident_pages());
                self.start_or_resume_target(Some(cx))
            }
            ExactGeometryProgress::TargetComplete => Err(RangeTextInputError::Stale),
            ExactGeometryProgress::PendingIndex => Err(RangeTextInputError::Stale),
        }
    }

    pub(super) fn retire_surface_candidate(&mut self) {
        let Some(candidate) = self.surface_candidate.take() else {
            return;
        };
        if self.active_geometry == Some(candidate.job) {
            self.active_geometry = None;
        }
        if let Ok(release) = self.geometry.cancel(candidate.job) {
            self.release_geometry(&release, None, None, None);
        }
    }

    pub(super) fn reject_restoration_geometry(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<bool, RangeTextInputError> {
        if self.restoration_seed.is_none() {
            return Ok(false);
        }
        let release = self
            .geometry
            .rebind(self.config.binding, self.config.presentation_generation)?;
        self.release_geometry(&release, None, None, Some(cx));
        Ok(self.reject_restoration_after_geometry_change(cx))
    }

    pub(super) fn reject_restoration_after_geometry_change(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.restoration_seed.is_none() {
            return false;
        }
        for key in self
            .object_residency
            .rebind(self.config.binding, self.config.presentation_generation)
        {
            self.cancel_object_page_dispatch(key);
        }
        self.active_geometry = None;
        self.pending_geometry_page = None;
        self.pending_geometry_object = None;
        self.surface_candidate = None;
        self.restoration_seed = None;
        self.published_restoration = None;
        self.desired = DesiredSurface::origin(self.config.viewport_extent, self.config.overscan);
        cx.emit(RangeTextInputEvent::RestorationRejected);
        cx.notify();
        true
    }

    pub(super) fn release_geometry(
        &mut self,
        release: &crate::ExactGeometryRelease,
        completed_page: Option<crate::PageRequestKey>,
        completed_object_page: Option<crate::ObjectRequestKey>,
        mut cx: Option<&mut Context<Self>>,
    ) {
        if self.pending_geometry_page.as_ref().is_some_and(|pending| {
            release.jobs.contains(&pending.job) || release.pages.contains(&pending.request.key())
        }) {
            self.pending_geometry_page = None;
        }
        if self
            .pending_geometry_object
            .as_ref()
            .is_some_and(|pending| {
                release.jobs.contains(&pending.job)
                    || release.object_pages.contains(&pending.request.key())
            })
        {
            self.pending_geometry_object = None;
        }
        if self
            .surface_candidate
            .as_ref()
            .is_some_and(|candidate| release.jobs.contains(&candidate.job))
        {
            self.surface_candidate = None;
        }
        for page in &release.pages {
            if Some(*page) == completed_page {
                continue;
            }
            let _ = self.residency.cancel(*page);
            self.cancel_page_dispatch(*page);
        }
        for page in &release.object_pages {
            if Some(*page) == completed_object_page {
                continue;
            }
            let _ = self.object_residency.cancel(*page);
            self.cancel_object_page_dispatch(*page);
        }
        if let Some(cx) = cx.as_mut() {
            cx.notify();
        }
    }

    pub(super) fn cancel_page_dispatch(&mut self, key: crate::PageRequestKey) {
        if let Some(index) = self.requests.iter().position(
            |request| matches!(request, RangeTextInputRequest::Page(page) if page.key() == key),
        ) {
            self.requests.remove(index);
        } else if self.dispatched_pages.remove(&key) {
            self.requests
                .push_back(RangeTextInputRequest::CancelPage(key));
        }
    }
}
