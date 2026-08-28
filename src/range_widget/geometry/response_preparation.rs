use super::*;

impl RangeTextInput {
    pub(in crate::range_widget) fn prepare_terminal_target_publication(
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
            None,
            self.residency.resident_page_iter(),
            self.object_residency.resident_page_iter(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_nonterminal_response_publication(
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
        index_target: Option<crate::range_widget::transition::PreparedIndexResponseTarget>,
    ) -> Result<PreparedNonterminalResponsePublication, RangeTextInputError> {
        if geometry.progress() == ExactGeometryProgress::TargetComplete
            || geometry.release().pages.as_slice() != consumed_page.as_slice()
            || geometry.release().object_pages.as_slice() != consumed_object_page.as_slice()
            || (index_target.is_none() && !geometry.release().jobs.is_empty())
            || completed_page.is_some() != text_admission.is_some()
            || completed_object_page.is_some() != object_admission.is_some()
        {
            return Err(RangeTextInputError::Stale);
        }
        let job = index_target
            .as_ref()
            .map_or_else(|| geometry.key(), |target| target.job);
        let successor = geometry.successor().ok_or(RangeTextInputError::Stale)?;
        let successor_is_index = match successor {
            crate::range_geometry::PreparedTargetSuccessor::Page(request) => {
                request.key().purpose() == crate::PagePurpose::GeometryIndex
            }
            crate::range_geometry::PreparedTargetSuccessor::Object { request, .. } => {
                request.key().purpose() == crate::ObjectPurpose::GeometryIndex
            }
        };
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
        let max_queued_requests = super::super::checked_request_capacity(&self.config)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        if destination_capacity > max_queued_requests {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        let destination_requests = VecDeque::with_capacity(max_queued_requests);
        if destination_requests.capacity() > max_queued_requests {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
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
                            .allocated_items()
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
                        .allocated_items()
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
        let current_realization_state = self.current_auxiliary_realization_charge()?;
        let current_request_payload = super::super::transition::queued_request_payload_charge(
            self.requests.iter(),
            self.clipboard.current_provenance_page(),
        )?;
        let destination_request_bytes = destination_requests
            .capacity()
            .checked_mul(size_of::<RangeTextInputRequest>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let surface_object_pages = self
            .surface
            .as_ref()
            .into_iter()
            .flat_map(|surface| surface.object_pages().iter());
        let presentation_overlap = if let Some(admission) = object_admission.as_ref() {
            geometry.presentation_overlap_bytes(
                surface_object_pages
                    .chain(admission.projected_resident_pages(&self.object_residency)),
            )
        } else {
            geometry.presentation_overlap_bytes(
                surface_object_pages.chain(self.object_residency.resident_page_iter()),
            )
        }
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let geometry_bytes = geometry
            .retained_bytes()
            .checked_sub(presentation_overlap)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let candidate_bytes = [
            size_of::<PreparedNonterminalResponsePublication>(),
            geometry_bytes,
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
            destination_requests.capacity(),
            usize::from(pending_page.is_some()),
            usize::from(pending_object.is_some()),
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let admission_charge = crate::RangeSurfaceCharge {
            bytes: Self::realization_owner_charge()
                .bytes
                .checked_add(prior.bytes)
                .and_then(|bytes| bytes.checked_add(current_request_bytes))
                .and_then(|bytes| bytes.checked_add(current_request_payload.bytes))
                .and_then(|bytes| bytes.checked_add(current_realization_state.bytes))
                .and_then(|bytes| bytes.checked_add(candidate_bytes))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: Self::realization_owner_charge()
                .items
                .checked_add(prior.items)
                .and_then(|items| items.checked_add(self.requests.capacity()))
                .and_then(|items| items.checked_add(current_request_payload.items))
                .and_then(|items| items.checked_add(current_realization_state.items))
                .and_then(|items| items.checked_add(candidate_items))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if admission_charge.bytes > self.config.limits.max_surface_bytes
            || admission_charge.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        let (next_id, desired, surface_candidate) = if let Some(target) = index_target {
            (
                target.committed_next_id,
                Some(target.desired),
                Some(target.surface_candidate),
            )
        } else {
            (
                self.next_id
                    .checked_add(if successor_is_index { 2 } else { 1 })
                    .ok_or(RangeTextInputError::Busy)?,
                None,
                None,
            )
        };
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
            effects,
            destination_requests,
            completed_page,
            completed_object_page,
            next_id,
            desired,
            surface_candidate,
        })
    }

    pub(super) fn prepare_target_publication_from<'a>(
        &self,
        target: &crate::BlockTargetPublication,
        mut state: SurfaceCandidate,
        projected_index: Option<&crate::ExactGeometryIndex>,
        pages: impl ExactSizeIterator<Item = &'a crate::RangePage> + Clone,
        object_pages: impl ExactSizeIterator<Item = &'a crate::ObjectPage> + Clone,
    ) -> Result<TerminalTargetPreparation, RangeTextInputError> {
        let index = projected_index.or_else(|| self.geometry.index());
        let (quality, visual_lines, content_height) = index.map_or_else(
            || {
                (
                    crate::GeometryQuality::Estimated,
                    target.visual_lines_lower_bound().max(1),
                    target
                        .content_height_lower_bound()
                        .max(self.config.layout.line_height),
                )
            },
            |index| {
                let aggregate = index.aggregate();
                (
                    crate::GeometryQuality::Exact,
                    aggregate.visual_lines(),
                    aggregate.content_height(),
                )
            },
        );
        if state.binding != self.config.binding
            || state.job != target.key()
            || state.job.geometry() != self.geometry.key()
        {
            return Err(RangeTextInputError::Stale);
        }
        if self.pending_select_all {
            let index = index.ok_or(RangeTextInputError::IncompleteSurface)?;
            state.desired.source_selection = Some(index.document_selection());
            state.desired.composition = None;
            state.desired.reveal_caret = true;
            state.desired.inline_object_interaction = self.active_object.map(|_| {
                crate::range_widget::DesiredInlineObjectInteraction::Clear(
                    crate::InlineObjectRealizationLossReason::SelectionChanged,
                )
            });
        }
        let desired = state.desired;
        let required_anchor = match desired.priority() {
            crate::RangeRealizationPriority::Caret
            | crate::RangeRealizationPriority::Ime
            | crate::RangeRealizationPriority::DirectedSelection
            | crate::RangeRealizationPriority::ActiveInteraction => desired
                .source_selection
                .map(|selection| selection.head.byte_offset),
            crate::RangeRealizationPriority::ScrollAnchor => desired
                .preserve_scroll_anchor
                .then_some(desired.scroll.source),
            crate::RangeRealizationPriority::NearbyContent => None,
        };
        if let Some(anchor) = required_anchor
            && (anchor < target.predecessor().byte_offset
                || anchor > target.source_end().byte_offset)
        {
            let mut retarget = desired;
            retarget.realization_anchor_block = if anchor < target.predecessor().byte_offset {
                index
                    .ok_or(RangeTextInputError::IncompleteSurface)?
                    .checkpoints()
                    .iter()
                    .rev()
                    .find(|checkpoint| checkpoint.source().byte_offset <= anchor)
                    .map(|checkpoint| checkpoint.block_offset())
                    .ok_or(RangeTextInputError::IncompleteSurface)?
            } else {
                desired.realization_anchor_block
                    + desired
                        .realization_extent
                        .max(self.config.layout.line_height)
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
            quality,
            visual_lines,
            content_height,
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

    pub(in crate::range_widget) fn resident_publication_payload_charge<'a>(
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
                        page.retained_charge()
                            .allocated_items()
                            .checked_add(1)
                            .ok_or(RangeTextInputError::SurfaceCapacity)?,
                    )
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
            })
        })
    }
}
