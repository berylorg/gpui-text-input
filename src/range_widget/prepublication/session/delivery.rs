use super::*;

pub(super) enum DeliveredResponse {
    Validation(RangePrepublicationValidationResponse),
    Page(RangePage),
    ObjectPage(ObjectPage),
}

impl DeliveredResponse {
    pub(super) fn charge(&self) -> Option<RangeSurfaceCharge> {
        match self {
            Self::Validation(_) => Some(RangeSurfaceCharge::default()),
            Self::Page(page) => Some(RangeSurfaceCharge {
                bytes: page
                    .retained_charge()
                    .bytes()
                    .checked_sub(std::mem::size_of::<RangePage>())?,
                items: page.retained_charge().items().checked_sub(1)?,
            }),
            Self::ObjectPage(page) => Some(RangeSurfaceCharge {
                bytes: page
                    .retained_charge()
                    .bytes()
                    .checked_sub(std::mem::size_of::<ObjectPage>())?,
                items: page.retained_charge().allocated_items(),
            }),
        }
    }
}

impl RangePrepublicationSession {
    pub fn deliver_validation(
        &mut self,
        response: RangePrepublicationValidationResponse,
    ) -> RangePrepublicationDelivery {
        let _ = self.environment.cleanup().mark_delivered_request(
            response.key.generation,
            CleanupRequest::Validation(response.key),
        );
        self.deliver(DeliveredResponse::Validation(response))
    }

    pub fn deliver_page(
        &mut self,
        generation: RangePrepublicationSessionGeneration,
        page: RangePage,
    ) -> RangePrepublicationDelivery {
        let _ = self.environment.cleanup().mark_delivered_request(
            generation,
            CleanupRequest::Page {
                generation,
                key: page.key(),
            },
        );
        if generation != self.generation {
            return RangePrepublicationDelivery::Obsolete;
        }
        self.deliver(DeliveredResponse::Page(page))
    }

    pub fn deliver_object_page(
        &mut self,
        generation: RangePrepublicationSessionGeneration,
        page: ObjectPage,
    ) -> RangePrepublicationDelivery {
        let _ = self.environment.cleanup().mark_delivered_request(
            generation,
            CleanupRequest::ObjectPage {
                generation,
                key: page.key(),
            },
        );
        if generation != self.generation {
            return RangePrepublicationDelivery::Obsolete;
        }
        self.deliver(DeliveredResponse::ObjectPage(page))
    }

    fn deliver(&mut self, response: DeliveredResponse) -> RangePrepublicationDelivery {
        if matches!(
            self.stage,
            SessionStage::Ready
                | SessionStage::Cancelled
                | SessionStage::Failed(_)
                | SessionStage::CandidateTaken
        ) {
            return RangePrepublicationDelivery::Obsolete;
        }
        if self.delivered.is_some() || self.admitted_geometry.is_some() {
            if self.response_matches_waiting(&response) {
                self.fail_without_effects(RangePrepublicationFailure::ExactKeyCollision);
                return RangePrepublicationDelivery::Terminal(
                    RangePrepublicationFailure::ExactKeyCollision,
                );
            }
            return RangePrepublicationDelivery::Obsolete;
        }
        if !self.response_matches_waiting(&response) {
            return RangePrepublicationDelivery::Obsolete;
        }
        self.delivered = Some(response);
        let Some(charge) = self.response_coexistence_charge() else {
            self.fail_without_effects(RangePrepublicationFailure::Arithmetic);
            return RangePrepublicationDelivery::Terminal(RangePrepublicationFailure::Arithmetic);
        };
        self.observe_charge(charge);
        let configured = configured_capacity(self.environment.config());
        if !charge_fits(charge, configured) {
            self.fail_without_effects(RangePrepublicationFailure::TerminalCapacity);
            return RangePrepublicationDelivery::Terminal(
                RangePrepublicationFailure::TerminalCapacity,
            );
        }
        if !charge_fits(charge, self.available) {
            return RangePrepublicationDelivery::CapacityBlocked;
        }
        RangePrepublicationDelivery::Accepted
    }

    fn response_matches_waiting(&self, response: &DeliveredResponse) -> bool {
        match (&self.waiting, response) {
            (Some(Waiting::Validation(request)), DeliveredResponse::Validation(response)) => {
                request.key == response.key
            }
            (Some(Waiting::RestorationPage { key, .. }), DeliveredResponse::Page(page)) => {
                *key == page.key()
            }
            (Some(Waiting::RestorationObject { key, .. }), DeliveredResponse::ObjectPage(page)) => {
                *key == page.key()
            }
            (Some(Waiting::GeometryPage { key, .. }), DeliveredResponse::Page(page)) => {
                *key == page.key()
            }
            (Some(Waiting::GeometryObject { key, .. }), DeliveredResponse::ObjectPage(page)) => {
                *key == page.key()
            }
            _ => false,
        }
    }

    pub(super) fn process_delivered(
        &mut self,
        _text_system: &WindowTextSystem,
        effects: &mut EffectBuffer,
    ) -> Result<bool, RangePrepublicationFailure> {
        let coexistence = add_charge(
            self.response_coexistence_charge()
                .ok_or(RangePrepublicationFailure::Arithmetic)?,
            multiply_charge(
                RangeSurfaceCharge {
                    bytes: std::mem::size_of::<RangePrepublicationEffect>(),
                    items: 1,
                },
                effects.len(),
            )
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
        )?;
        let mut object_storage = None;
        let storage = if let Some(DeliveredResponse::Page(page)) = self.delivered.as_ref() {
            if !matches!(
                self.waiting,
                Some(Waiting::RestorationPage { .. } | Waiting::GeometryPage { .. })
            ) {
                return Err(RangePrepublicationFailure::Stale);
            }
            let configured = configured_capacity(self.environment.config());
            let available = self.available;
            let mut peak = coexistence;
            let mut failure = None;
            let prepared = self.residency.prepare_admit_storage(page, |bytes, items| {
                let charge = match add_charge(coexistence, RangeSurfaceCharge { bytes, items }) {
                    Ok(charge) => charge,
                    Err(error) => {
                        failure = Some(error);
                        return false;
                    }
                };
                peak.bytes = peak.bytes.max(charge.bytes);
                peak.items = peak.items.max(charge.items);
                if !charge_fits(charge, configured) {
                    failure = Some(RangePrepublicationFailure::TerminalCapacity);
                    return false;
                }
                charge_fits(charge, available)
            });
            self.observe_charge(peak);
            if let Some(failure) = failure {
                return Err(failure);
            }
            let Some(prepared) = prepared.map_err(classify_page_admission)? else {
                self.ledger_blocked = true;
                return Ok(false);
            };
            Some(prepared)
        } else if let Some(DeliveredResponse::ObjectPage(page)) = self.delivered.as_ref() {
            if !matches!(
                self.waiting,
                Some(Waiting::RestorationObject { .. } | Waiting::GeometryObject { .. })
            ) {
                return Err(RangePrepublicationFailure::Stale);
            }
            let configured = configured_capacity(self.environment.config());
            let available = self.available;
            let mut peak = coexistence;
            let mut failure = None;
            let prepared = self.object_residency.prepare_delivered_storage(
                page,
                &self.residency,
                |bytes, items| {
                    let charge = match add_charge(coexistence, RangeSurfaceCharge { bytes, items })
                    {
                        Ok(charge) => charge,
                        Err(error) => {
                            failure = Some(error);
                            return false;
                        }
                    };
                    peak.bytes = peak.bytes.max(charge.bytes);
                    peak.items = peak.items.max(charge.items);
                    if !charge_fits(charge, configured) {
                        failure = Some(RangePrepublicationFailure::TerminalCapacity);
                        return false;
                    }
                    charge_fits(charge, available)
                },
            );
            self.observe_charge(peak);
            if let Some(failure) = failure {
                return Err(failure);
            }
            let Some(prepared) = prepared.map_err(classify_object_admission)? else {
                self.ledger_blocked = true;
                return Ok(false);
            };
            object_storage = Some(prepared);
            None
        } else {
            if !charge_fits(coexistence, self.available) {
                return Ok(false);
            }
            None
        };
        let response = self.delivered.take().expect("delivered response checked");
        let waiting = self
            .waiting
            .take()
            .ok_or(RangePrepublicationFailure::Stale)?;
        match (waiting, response) {
            (Waiting::Validation(request), DeliveredResponse::Validation(response)) => {
                if response.key != request.key
                    || !response.current
                    || response.binding != request.binding
                {
                    return Err(RangePrepublicationFailure::Stale);
                }
                validation_matches_seed(self.seed, response.binding, response.history)?;
                self.accepted_validation = Some(response);
                self.stage = SessionStage::Restoration;
                if !self.environment.cleanup().complete(request.cleanup) {
                    return Err(RangePrepublicationFailure::Stale);
                }
            }
            (Waiting::RestorationPage { cleanup, .. }, DeliveredResponse::Page(page)) => {
                let page_id = match self
                    .residency
                    .commit_prepared_admit(storage.expect("text storage prepared").with_page(page))
                {
                    crate::PageAdmission::Admitted { page, .. } => page,
                };
                let page = self
                    .residency
                    .peek_page_by_id(page_id)
                    .ok_or(RangePrepublicationFailure::MalformedResponse)?;
                self.validation
                    .accept_text(page)
                    .map_err(classify_widget_error)?;
                self.retain_text_custody(page_id, cleanup)?;
            }
            (Waiting::RestorationObject { cleanup, .. }, DeliveredResponse::ObjectPage(page)) => {
                let page_id = object_page_id(
                    self.object_residency.commit_prepared_admit(
                        object_storage
                            .expect("object storage prepared")
                            .with_page(page),
                    ),
                );
                let page = self
                    .object_residency
                    .peek_page_by_id(page_id)
                    .ok_or(RangePrepublicationFailure::MalformedResponse)?;
                self.validation
                    .accept_object(page)
                    .map_err(classify_widget_error)?;
                self.retain_object_custody(page_id, cleanup)?;
            }
            (Waiting::GeometryPage { job, cleanup, .. }, DeliveredResponse::Page(page)) => {
                let page_id = match self
                    .residency
                    .commit_prepared_admit(storage.expect("text storage prepared").with_page(page))
                {
                    crate::PageAdmission::Admitted { page, .. } => page,
                };
                self.retain_text_custody(page_id, cleanup)?;
                self.waiting = Some(waiting);
                self.retain_geometry_response(GeometryResponseInput::Page {
                    job,
                    page: page_id,
                    resident: false,
                })?;
            }
            (
                Waiting::GeometryObject {
                    job,
                    text_page,
                    cleanup,
                    ..
                },
                DeliveredResponse::ObjectPage(page),
            ) => {
                let page_id = object_page_id(
                    self.object_residency.commit_prepared_admit(
                        object_storage
                            .expect("object storage prepared")
                            .with_page(page),
                    ),
                );
                self.retain_object_custody(page_id, cleanup)?;
                self.waiting = Some(waiting);
                self.retain_geometry_response(GeometryResponseInput::Object {
                    job,
                    text_page,
                    page: page_id,
                    resident: false,
                })?;
            }
            _ => return Err(RangePrepublicationFailure::Stale),
        }
        Ok(true)
    }
}
