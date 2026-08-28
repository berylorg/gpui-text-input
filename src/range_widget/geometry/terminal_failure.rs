use super::*;

pub(super) enum TerminalResponsePreparation {
    Publication(PreparedTerminalResponsePublication),
    Failure(PreparedTerminalResponseFailure),
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

impl RangeTextInput {
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
            (Some(key), None) => self.geometry.prepare_terminal_page_failure(job, key)?,
            (None, Some(key)) => self.geometry.prepare_terminal_object_failure(job, key)?,
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

    pub(super) fn prepare_terminal_object_response_failure(
        &self,
        job: crate::GeometryJobKey,
        key: ObjectRequestKey,
        error: RangeTextInputError,
    ) -> Result<TerminalResponsePreparation, RangeTextInputError> {
        self.prepare_terminal_response_failure(job, None, Some(key), false, true, error)
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
    ) -> Result<TerminalResponsePreparation, RangeTextInputError> {
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
            Err(RangeTextInputError::SurfaceCapacity) => Err(RangeTextInputError::SurfaceCapacity),
            Err(_error) => self.prepare_terminal_response_failure(
                job,
                completed_page,
                completed_object_page,
                delivered_page,
                delivered_object_page,
                RangeTextInputError::IncompleteSurface,
            ),
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
