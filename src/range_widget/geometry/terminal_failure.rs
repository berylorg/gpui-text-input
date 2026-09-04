use super::*;

pub(super) enum TerminalResponsePreparation {
    Publication(PreparedTerminalResponsePublication),
    Failure(PreparedTerminalResponseFailure),
}

pub(super) enum TerminalResponsePreparationError {
    RetryablePublicationCapacity,
    Error(RangeTextInputError),
}

impl TerminalResponsePreparationError {
    pub(super) fn into_range_error(self) -> RangeTextInputError {
        match self {
            Self::RetryablePublicationCapacity => RangeTextInputError::SurfaceCapacity,
            Self::Error(error) => error,
        }
    }
}

pub(super) struct PreparedTerminalResponseFailure {
    geometry: crate::range_geometry::PreparedTerminalGeometryFailure,
    completed_page: Option<PageRequestKey>,
    completed_object_page: Option<ObjectRequestKey>,
    delivered_page: bool,
    delivered_object_page: bool,
    error: RangeTextInputError,
    release_request: Option<RangeTextInputRequest>,
    destination_requests: VecDeque<RangeTextInputRequest>,
}

pub(super) struct PreparedResidencyObjectResponseFailure {
    job: crate::GeometryJobKey,
    pending_key: ObjectRequestKey,
    response_key: ObjectRequestKey,
    settlement: ResidencyObjectResponseFailureSettlement,
    error: RangeTextInputError,
    destination_requests: VecDeque<RangeTextInputRequest>,
}

#[derive(Clone, Copy)]
enum ResidencyObjectResponseFailureSettlement {
    ActiveCoalesced { schedule_continuation: bool },
    DetachedSuperseded,
}

impl RangeTextInput {
    pub(super) fn settle_terminal_page_response(
        &mut self,
        job: crate::GeometryJobKey,
        key: PageRequestKey,
        error: RangeTextInputError,
        cx: &mut Context<Self>,
    ) -> Result<super::super::response_custody::ResponseDeliveryProgress, RangeTextInputError> {
        let preparation =
            self.prepare_terminal_response_failure(job, Some(key), None, true, false, error)?;
        let error = self
            .commit_terminal_response_preparation(preparation, cx)
            .expect_err("terminal failure preparation returns its accepted terminal outcome");
        Ok(super::super::response_custody::ResponseDeliveryProgress::AcceptedTerminal(error))
    }

    pub(super) fn settle_terminal_object_response(
        &mut self,
        job: crate::GeometryJobKey,
        key: ObjectRequestKey,
        error: RangeTextInputError,
        cx: &mut Context<Self>,
    ) -> Result<super::super::response_custody::ResponseDeliveryProgress, RangeTextInputError> {
        let preparation =
            self.prepare_terminal_response_failure(job, None, Some(key), false, true, error)?;
        let error = self
            .commit_terminal_response_preparation(preparation, cx)
            .expect_err("terminal failure preparation returns its accepted terminal outcome");
        Ok(super::super::response_custody::ResponseDeliveryProgress::AcceptedTerminal(error))
    }

    pub(super) fn reject_terminal_page_response(
        &mut self,
        job: crate::GeometryJobKey,
        key: PageRequestKey,
        error: RangeTextInputError,
        cx: &mut Context<Self>,
    ) -> Result<super::super::response_custody::ResponseDeliveryProgress, RangeTextInputError> {
        let preparation =
            self.prepare_terminal_response_failure(job, Some(key), None, true, false, error)?;
        let error = self
            .commit_terminal_response_preparation(preparation, cx)
            .expect_err("terminal rejection preparation returns its public error");
        Ok(super::super::response_custody::ResponseDeliveryProgress::Rejected(error))
    }

    pub(super) fn reject_terminal_object_response(
        &mut self,
        job: crate::GeometryJobKey,
        key: ObjectRequestKey,
        error: RangeTextInputError,
        cx: &mut Context<Self>,
    ) -> Result<super::super::response_custody::ResponseDeliveryProgress, RangeTextInputError> {
        let preparation =
            self.prepare_terminal_response_failure(job, None, Some(key), false, true, error)?;
        let error = self
            .commit_terminal_response_preparation(preparation, cx)
            .expect_err("terminal rejection preparation returns its public error");
        Ok(super::super::response_custody::ResponseDeliveryProgress::Rejected(error))
    }

    pub(super) fn prepare_residency_object_response_failure(
        &self,
        job: crate::GeometryJobKey,
        pending_key: ObjectRequestKey,
        response_key: ObjectRequestKey,
        detached_job: bool,
        error: RangeTextInputError,
    ) -> Result<PreparedResidencyObjectResponseFailure, RangeTextInputError> {
        let pending = self
            .pending_geometry_object
            .as_ref()
            .ok_or(RangeTextInputError::Stale)?;
        if !detached_job && pending_key == response_key {
            return Err(RangeTextInputError::Stale);
        }
        if pending.job != job
            || pending.request.key() != pending_key
            || !matches!(pending.wait, GeometryObjectWait::Coalesced(key) if key == response_key)
            || !self.dispatched_object_pages.contains(&response_key)
            || detached_job != (self.active_geometry != Some(job))
        {
            return Err(RangeTextInputError::Stale);
        }
        let maximum = super::super::checked_request_capacity(&self.config)
            .expect("constructed range widget retains a valid request capacity");
        let retired_cancel_count = self
            .requests
            .iter()
            .filter(|request| {
                matches!(request, RangeTextInputRequest::CancelObjectPage(key) if *key == response_key)
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
            .expect("bounded residency response cleanup request count remains representable");
        if required > maximum {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        let settlement = if detached_job {
            ResidencyObjectResponseFailureSettlement::DetachedSuperseded
        } else {
            ResidencyObjectResponseFailureSettlement::ActiveCoalesced {
                schedule_continuation: required < maximum,
            }
        };
        let destination_requests = VecDeque::with_capacity(maximum);
        if destination_requests.capacity() > maximum {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        Ok(PreparedResidencyObjectResponseFailure {
            job,
            pending_key,
            response_key,
            settlement,
            error,
            destination_requests,
        })
    }

    pub(super) fn commit_residency_object_response_failure(
        &mut self,
        prepared: PreparedResidencyObjectResponseFailure,
        cx: &mut Context<Self>,
    ) -> RangeTextInputError {
        let PreparedResidencyObjectResponseFailure {
            job,
            pending_key,
            response_key,
            settlement,
            error,
            mut destination_requests,
        } = prepared;
        let pending = self
            .pending_geometry_object
            .as_ref()
            .expect("prepared residency response failure retains its pending input");
        debug_assert_eq!(pending.job, job);
        debug_assert_eq!(pending.request.key(), pending_key);
        debug_assert!(
            matches!(pending.wait, GeometryObjectWait::Coalesced(key) if key == response_key)
        );
        debug_assert!(match settlement {
            ResidencyObjectResponseFailureSettlement::ActiveCoalesced { .. } => {
                pending_key != response_key && self.active_geometry == Some(job)
            }
            ResidencyObjectResponseFailureSettlement::DetachedSuperseded => {
                self.active_geometry != Some(job)
            }
        });

        let prior_requests = std::mem::take(&mut self.requests);
        destination_requests.extend(prior_requests.into_iter().filter(|request| {
            !matches!(request, RangeTextInputRequest::CancelObjectPage(key) if *key == response_key)
        }));
        destination_requests.push_back(RangeTextInputRequest::ReleaseObjectPage(response_key));
        debug_assert!(destination_requests.len() <= destination_requests.capacity());
        self.requests = destination_requests;

        let _ = self
            .object_residency
            .settle(response_key, crate::ObjectPageFailure::Unavailable);
        assert!(self.dispatched_object_pages.remove(&response_key));
        match settlement {
            ResidencyObjectResponseFailureSettlement::ActiveCoalesced {
                schedule_continuation,
            } => {
                if schedule_continuation {
                    self.schedule_realization_continuation(cx);
                }
            }
            ResidencyObjectResponseFailureSettlement::DetachedSuperseded => {
                self.pending_geometry_object = None;
                self.superseded_geometry_object_responses_settled = self
                    .superseded_geometry_object_responses_settled
                    .saturating_add(1);
            }
        }
        self.observe_realization_ownership();
        cx.notify();
        error
    }

    fn prepare_terminal_response_failure(
        &self,
        job: crate::GeometryJobKey,
        completed_page: Option<PageRequestKey>,
        completed_object_page: Option<ObjectRequestKey>,
        delivered_page: bool,
        delivered_object_page: bool,
        error: RangeTextInputError,
    ) -> Result<TerminalResponsePreparation, RangeTextInputError> {
        let geometry = match (completed_page, completed_object_page) {
            (Some(_), None) | (None, Some(_)) => self
                .geometry
                .prepare_terminal_failure_for_active_input(job)?,
            _ => unreachable!("terminal response names exactly one completed input"),
        };
        let release_request = match (delivered_page, delivered_object_page) {
            (true, false) => completed_page.map(RangeTextInputRequest::ReleasePage),
            (false, true) => completed_object_page.map(RangeTextInputRequest::ReleaseObjectPage),
            _ => None,
        };
        let maximum = super::super::checked_request_capacity(&self.config)
            .expect("constructed range widget retains a valid request capacity");
        let required = self
            .requests
            .len()
            .checked_add(usize::from(release_request.is_some()))
            .expect("bounded terminal cleanup request count remains representable");
        assert!(
            required <= maximum,
            "range widget reserves request capacity for terminal cleanup"
        );
        let destination_requests = VecDeque::with_capacity(maximum);
        assert!(
            destination_requests.capacity() <= maximum,
            "terminal cleanup destination exceeds the configured request capacity"
        );
        Ok(TerminalResponsePreparation::Failure(
            PreparedTerminalResponseFailure {
                geometry,
                completed_page,
                completed_object_page,
                delivered_page,
                delivered_object_page,
                error,
                release_request,
                destination_requests,
            },
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_terminal_response_publication(
        &self,
        geometry: crate::range_geometry::PreparedTargetResponse,
        text_admission: Option<crate::residency::PreparedRangePageAdmission>,
        object_admission: Option<crate::object_residency::PreparedObjectPageAdmission>,
        text_touch: Option<PageId>,
        object_touch: Option<ObjectPageId>,
        completed_page: Option<PageRequestKey>,
        completed_object_page: Option<ObjectRequestKey>,
    ) -> Result<TerminalResponsePreparation, TerminalResponsePreparationError> {
        let job = geometry.key();
        let delivered_page = text_admission.is_some();
        let delivered_object_page = object_admission.is_some();
        let result = (|| {
            let index_target = geometry
                .terminal_index()
                .map(|_| self.prepare_index_response_target(&geometry))
                .transpose()?;
            self.try_prepare_terminal_response_publication(
                geometry,
                text_admission,
                object_admission,
                text_touch,
                object_touch,
                completed_page,
                completed_object_page,
                index_target,
            )
        })();
        match result {
            Ok(publication) => Ok(TerminalResponsePreparation::Publication(publication)),
            Err(response_commit::TerminalPublicationPreparationError::PublicationCapacity)
                if self.surface.is_some() =>
            {
                Err(TerminalResponsePreparationError::RetryablePublicationCapacity)
            }
            Err(response_commit::TerminalPublicationPreparationError::PublicationCapacity) => self
                .prepare_terminal_response_failure(
                    job,
                    completed_page,
                    completed_object_page,
                    delivered_page,
                    delivered_object_page,
                    RangeTextInputError::SurfaceCapacity,
                )
                .map_err(TerminalResponsePreparationError::Error),
            Err(response_commit::TerminalPublicationPreparationError::Error(error)) => self
                .prepare_terminal_response_failure(
                    job,
                    completed_page,
                    completed_object_page,
                    delivered_page,
                    delivered_object_page,
                    error,
                )
                .map_err(TerminalResponsePreparationError::Error),
        }
    }

    pub(super) fn commit_terminal_response_preparation(
        &mut self,
        preparation: TerminalResponsePreparation,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        match preparation {
            TerminalResponsePreparation::Publication(publication) => {
                self.commit_terminal_response_publication(publication, cx);
                Ok(())
            }
            TerminalResponsePreparation::Failure(failure) => {
                self.commit_terminal_response_failure(failure, cx)
            }
        }
    }

    fn commit_terminal_response_failure(
        &mut self,
        prepared: PreparedTerminalResponseFailure,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let PreparedTerminalResponseFailure {
            geometry,
            completed_page,
            completed_object_page,
            delivered_page,
            delivered_object_page,
            error,
            release_request,
            mut destination_requests,
        } = prepared;
        let prior_requests = std::mem::take(&mut self.requests);
        destination_requests.extend(prior_requests);
        if let Some(release_request) = release_request {
            destination_requests.push_back(release_request);
        }
        debug_assert!(destination_requests.len() <= destination_requests.capacity());
        self.requests = destination_requests;
        let release = self.geometry.commit_prepared_terminal_failure(geometry);
        if delivered_page {
            let key = completed_page.expect("delivered text failure names its response");
            let _ = self.residency.settle(key, crate::PageFailure::Unavailable);
            assert!(self.dispatched_pages.remove(&key));
        }
        if delivered_object_page {
            let key = completed_object_page.expect("delivered object failure names its response");
            let _ = self
                .object_residency
                .settle(key, crate::ObjectPageFailure::Unavailable);
            assert!(self.dispatched_object_pages.remove(&key));
        }
        self.release_geometry(&release, completed_page, completed_object_page, None);
        self.active_geometry = None;
        self.pending_target_intent = None;
        self.pending_index_intent = false;
        self.observe_realization_ownership();
        cx.notify();
        Err(error)
    }
}
