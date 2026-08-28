use super::*;

impl RangeTextInput {
    pub(super) fn try_prepare_terminal_response_publication(
        &self,
        geometry: crate::range_geometry::PreparedTargetResponse,
        text_admission: Option<crate::residency::PreparedRangePageAdmission>,
        object_admission: Option<crate::object_residency::PreparedObjectPageAdmission>,
        text_touch: Option<PageId>,
        object_touch: Option<ObjectPageId>,
        completed_page: Option<PageRequestKey>,
        completed_object_page: Option<ObjectRequestKey>,
        index_target: Option<crate::range_widget::transition::PreparedIndexResponseTarget>,
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
        let projected_index = geometry.terminal_index();
        let (state, next_id) = if let Some(index_target) = index_target {
            if index_target.surface_candidate.job != target.key() {
                return Err(RangeTextInputError::Stale);
            }
            (
                index_target.surface_candidate,
                Some(index_target.committed_next_id),
            )
        } else {
            (
                *self
                    .surface_candidate
                    .as_ref()
                    .filter(|state| state.job == target.key())
                    .ok_or(RangeTextInputError::Stale)?,
                None,
            )
        };
        let preparation = match (&text_admission, &object_admission) {
            (Some(text), None) => self.prepare_target_publication_from(
                target,
                state,
                projected_index,
                text.projected_resident_pages(&self.residency),
                self.object_residency
                    .resident_pages_after_touch(object_touch),
            )?,
            (None, Some(objects)) => self.prepare_target_publication_from(
                target,
                state,
                projected_index,
                self.residency.resident_pages_after_touch(text_touch),
                objects.projected_resident_pages(&self.object_residency),
            )?,
            (None, None) => self.prepare_target_publication_from(
                target,
                state,
                projected_index,
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
        let current_realization_state = self.current_auxiliary_realization_charge()?;
        let current_request_payload =
            super::super::transition::queued_request_payload_charge(self.requests.iter())?;
        let destination_request_bytes = destination_requests
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
            destination_requests.capacity(),
            usize::from(publication.active_loss().is_some()),
            usize::from(publication.activation().is_some()),
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
        let final_publication_charge = crate::RangeSurfaceCharge {
            bytes: Self::realization_owner_charge()
                .bytes
                .checked_add(publication.final_charge().bytes)
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: Self::realization_owner_charge()
                .items
                .checked_add(publication.final_charge().items)
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if admission_charge.bytes > self.config.limits.max_surface_bytes
            || admission_charge.items > self.config.limits.max_surface_items
            || final_publication_charge.bytes > self.config.limits.max_surface_bytes
            || final_publication_charge.items > self.config.limits.max_surface_items
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
            release_request,
            destination_requests,
            completed_page,
            completed_object_page,
            admission_charge,
            next_id,
        })
    }

    pub(in crate::range_widget) fn commit_prepared_target_publication(
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
        self.observe_surface_admission_peak(admission);
        if let Some(active_result) = active_result {
            self.install_active_object(active_result);
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
        self.desired.realization_anchor_block = state.desired.realization_anchor_block;
        self.desired.preserve_scroll_anchor = false;
        self.desired.reveal_caret = state.desired.reveal_caret;
        self.desired.inline_object_interaction = None;
        if let Some(selection) = select_all {
            self.pending_select_all = false;
            self.desired.source_selection = Some(selection);
            self.desired.reveal_caret = false;
        }
    }

    pub(super) fn commit_terminal_response_publication(
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
            release_request,
            mut destination_requests,
            completed_page,
            completed_object_page,
            admission_charge,
            next_id,
        } = candidate;
        let prior_requests = std::mem::take(&mut self.requests);
        destination_requests.extend(prior_requests);
        if let Some(release_request) = release_request {
            destination_requests.push_back(release_request);
        }
        debug_assert!(destination_requests.len() <= destination_requests.capacity());
        self.requests = destination_requests;
        let active_loss = publication.active_loss();
        let activation = publication.activation();
        let admission = self.geometry.commit_prepared_target_response(geometry);
        debug_assert_eq!(admission.progress(), ExactGeometryProgress::TargetComplete);
        if self.geometry.index().is_none() {
            self.pending_index_intent = true;
        }
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
        if let Some(next_id) = next_id {
            self.next_id = next_id;
        }
        self.commit_prepared_target_publication(publication, admission_charge);
        if let Some(loss) = active_loss {
            cx.emit(RangeTextInputEvent::InlineObjectRealizationLost(loss));
        }
        if let Some(activation) = activation {
            cx.emit(RangeTextInputEvent::InlineObjectActivated(activation));
        }
        self.observe_realization_ownership();
        cx.notify();
    }

    pub(super) fn commit_nonterminal_response_publication(
        &mut self,
        candidate: PreparedNonterminalResponsePublication,
        cx: &mut Context<Self>,
    ) {
        let committed_job = candidate
            .surface_candidate
            .as_ref()
            .map_or_else(|| candidate.geometry.key(), |surface| surface.job);
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
            effects,
            mut destination_requests,
            completed_page,
            completed_object_page,
            next_id,
            desired,
            surface_candidate,
        } = candidate;
        let prior_requests = std::mem::take(&mut self.requests);
        destination_requests.extend(prior_requests);
        for effect in effects.into_iter().flatten() {
            destination_requests.push_back(effect);
        }
        debug_assert!(destination_requests.len() <= destination_requests.capacity());
        self.requests = destination_requests;
        let admission = self.geometry.commit_prepared_target_response(geometry);
        debug_assert_ne!(admission.progress(), ExactGeometryProgress::TargetComplete);
        self.active_geometry = Some(committed_job);
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
        if let Some(desired) = desired {
            self.desired = desired;
        }
        if let Some(surface_candidate) = surface_candidate {
            self.surface_candidate = Some(surface_candidate);
        }
        self.next_id = next_id;
        self.observe_realization_ownership();
        cx.notify();
    }
}
