use std::sync::Arc;

use super::*;

impl ExactGeometryOwner {
    pub fn request_block_target(
        &mut self,
        id: GeometryJobId,
        target: BlockTarget,
    ) -> Result<ExactGeometryStart, ExactGeometryError> {
        validate_target(target)?;
        self.admit_job_id(id)?;
        let key = GeometryJobKey::new(self.key, id);
        if self.index.is_none()
            || self
                .active
                .as_deref()
                .is_some_and(|active| matches!(active.kind, ActiveKind::Index))
        {
            let required = self
                .counts()
                .total_bytes()
                .saturating_add(std::mem::size_of::<DesiredTarget>());
            let required_items = self.counts().total_items().saturating_add(1);
            if required > self.limits.max_retained_bytes
                || required_items > self.limits.max_retained_items
            {
                return Err(ExactGeometryError::CapacityExceeded);
            }
            let prior = self
                .desired_target
                .replace(Box::new(DesiredTarget { key, target }));
            if let Err(error) = self.refresh_active_capacity() {
                self.desired_target = prior;
                let _ = self.refresh_active_capacity();
                return Err(error);
            }
            self.highest_job = Some(id);
            self.high_water_bytes = self.high_water_bytes.max(required);
            self.high_water_items = self.high_water_items.max(required_items);
            self.observe_current();
            let release = prior.map_or_else(ExactGeometryRelease::default, owner::desired_release);
            return Ok(ExactGeometryStart {
                key,
                progress: ExactGeometryProgress::PendingIndex,
                release,
                admission_required_bytes: required,
                admission_required_items: required_items,
            });
        }
        if self.active.is_some() {
            return Err(ExactGeometryError::Busy);
        }
        let start = self.start_target(key, target)?;
        self.highest_job = Some(id);
        Ok(start)
    }

    pub fn start_pending_target(&mut self) -> Result<ExactGeometryStart, ExactGeometryError> {
        if self.active.is_some() {
            return Err(ExactGeometryError::Busy);
        }
        let desired = self
            .desired_target
            .take()
            .ok_or(ExactGeometryError::NoActiveJob)?;
        if self.index.is_none() {
            self.desired_target = Some(desired);
            return Err(ExactGeometryError::IndexIncomplete);
        }
        let DesiredTarget { key, target } = *desired;
        let start = match self.start_target(key, target) {
            Ok(start) => start,
            Err(error) => {
                self.desired_target = Some(Box::new(DesiredTarget { key, target }));
                return Err(error);
            }
        };
        Ok(start)
    }

    fn start_target(
        &mut self,
        key: GeometryJobKey,
        target: BlockTarget,
    ) -> Result<ExactGeometryStart, ExactGeometryError> {
        let predecessor = self
            .index
            .as_deref()
            .ok_or(ExactGeometryError::IndexIncomplete)?
            .checkpoints
            .iter()
            .rev()
            .find(|checkpoint| {
                checkpoint.source.get() == 0
                    || checkpoint.resume_block_offset() <= target.block_offset
            })
            .expect("index has origin")
            .clone();
        let inputs = self.inputs()?;
        let source_len = inputs.binding.extent().byte_len();
        if predecessor.source.get() == source_len {
            let candidate = BlockTargetPublication {
                key,
                predecessor: predecessor.source,
                target_source: predecessor.source,
                source_end: predecessor.source,
                fragments: Arc::from([]),
                charge: Default::default(),
                item_charge: Default::default(),
            };
            let counts = accounting::counts_with_target_candidate(self, &candidate);
            let required = counts
                .total_bytes()
                .saturating_add(std::mem::size_of::<ExactGeometryCheckpoint>());
            let required_items = counts.total_items().saturating_add(1);
            if required > self.limits.max_retained_bytes
                || required_items > self.limits.max_retained_items
            {
                return Err(ExactGeometryError::CapacityExceeded);
            }
            self.high_water_bytes = self.high_water_bytes.max(required);
            self.high_water_items = self.high_water_items.max(required_items);
            let prior = self.target.replace(Box::new(candidate));
            self.observe_current();
            return Ok(ExactGeometryStart {
                key,
                progress: ExactGeometryProgress::TargetComplete,
                release: prior
                    .map_or_else(ExactGeometryRelease::default, admission::target_release),
                admission_required_bytes: required,
                admission_required_items: required_items,
            });
        }
        let fixed = accounting::fixed_bytes_without_active(self);
        let retained_capacity = self
            .limits
            .max_retained_bytes
            .checked_sub(fixed)
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        let mut active = ActiveJob {
            key,
            kind: ActiveKind::Target {
                target,
                predecessor: predecessor.source,
            },
            page_use: ActivePageUse::Traverse {
                anchor: predecessor.source,
            },
            pending: None,
            window_identity: None,
            retained_capacity,
            scanner: Scanner::from_checkpoint(&predecessor),
        };
        accounting::ensure_active(&mut active)?;
        let required = fixed
            .saturating_add(accounting::active_bytes(&active))
            .saturating_add(std::mem::size_of::<ExactGeometryCheckpoint>());
        let required_items = accounting::fixed_counts_without_active(self)
            .total_items()
            .saturating_add(accounting::active_counts(&active).total_items())
            .saturating_add(1);
        if required > self.limits.max_retained_bytes
            || required_items > self.limits.max_retained_items
        {
            return Err(ExactGeometryError::CapacityExceeded);
        }
        self.active = Some(Box::new(active));
        self.high_water_bytes = self.high_water_bytes.max(required);
        self.high_water_items = self.high_water_items.max(required_items);
        self.observe_current();
        Ok(ExactGeometryStart {
            key,
            progress: ExactGeometryProgress::Scanning,
            release: ExactGeometryRelease::default(),
            admission_required_bytes: required,
            admission_required_items: required_items,
        })
    }
}

fn validate_target(target: BlockTarget) -> Result<(), ExactGeometryError> {
    let values = [target.block_offset, target.viewport_extent, target.overscan];
    let end = target.block_offset + target.viewport_extent + target.overscan;
    if values
        .iter()
        .any(|value| !f32::from(*value).is_finite() || *value < gpui::Pixels::ZERO)
        || !f32::from(end).is_finite()
    {
        Err(ExactGeometryError::InvalidMetric)
    } else {
        Ok(())
    }
}
