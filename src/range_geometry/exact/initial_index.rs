use super::*;

pub(crate) struct PreparedInitialIndex {
    active: Box<ActiveJob>,
    peak: (usize, usize),
    enclosing_peak: (usize, usize),
}

impl PreparedInitialIndex {
    pub(crate) fn enclosing_peak(&self) -> (usize, usize) {
        self.enclosing_peak
    }
}

impl ExactGeometryOwner {
    pub(crate) fn prepare_initial_index(
        &self,
        id: GeometryJobId,
        capacity: ResponseCapacity,
    ) -> Result<PreparedInitialIndex, ExactGeometryFailure> {
        self.admit_job_id(id)
            .map_err(|error| self.prepared_validation_failure(error))?;
        if self.active.is_some() {
            return Err(self.prepared_validation_failure(ExactGeometryError::Busy));
        }
        let inputs = self
            .inputs()
            .map_err(|error| self.prepared_validation_failure(error))?;
        let source_len = usize::try_from(inputs.binding.extent().byte_len())
            .map_err(|_| self.prepared_validation_failure(ExactGeometryError::SourceContract))?;
        let mut budget = self.prepared_budget(0, 0, capacity)?;
        let retained_capacity = self
            .limits
            .max_retained_bytes
            .checked_sub(budget.fixed_bytes)
            .ok_or_else(|| {
                self.prepared_validation_failure(ExactGeometryError::CapacityExceeded)
            })?;
        let (scanner, origin) = Scanner::origin_unallocated(&inputs.layout, source_len);
        let mut active = ActiveJob {
            key: GeometryJobKey::new(self.key, id),
            kind: ActiveKind::Index,
            page_use: ActivePageUse::Traverse {
                anchor: ByteOffset::new(0),
            },
            pending: None,
            text_page: None,
            window_identity: None,
            retained_capacity,
            scanner,
        };
        checkpoint::retain_scanner_checkpoint(
            &mut active,
            origin,
            self.limits.max_checkpoints,
            &mut budget,
            0,
            0,
        )
        .map_err(|error| ExactGeometryFailure {
            capacity_refusal: budget.observations.as_ref().and_then(|value| value.refusal),
            enclosing_peak: budget
                .observations
                .as_ref()
                .map(|value| value.enclosing_peak),
            error,
            stage: ExactGeometryFailureStage::Checkpoint,
            release: ExactGeometryRelease::default(),
            admission_required_bytes: budget.peak_bytes,
            admission_required_items: budget.peak_items,
        })?;
        Ok(PreparedInitialIndex {
            active: Box::new(active),
            peak: (budget.peak_bytes, budget.peak_items),
            enclosing_peak: budget.observations.as_ref().unwrap().enclosing_peak,
        })
    }

    pub(crate) fn commit_initial_index(
        &mut self,
        prepared: PreparedInitialIndex,
    ) -> ExactGeometryStart {
        let key = prepared.active.key;
        self.highest_job = Some(key.job());
        self.active = Some(prepared.active);
        self.observe_current();
        self.high_water_bytes = self.high_water_bytes.max(prepared.peak.0);
        self.high_water_items = self.high_water_items.max(prepared.peak.1);
        let mut result = self.start_result(key, ExactGeometryProgress::Scanning);
        result.admission_required_bytes = prepared.peak.0;
        result.admission_required_items = prepared.peak.1;
        result
    }
}
