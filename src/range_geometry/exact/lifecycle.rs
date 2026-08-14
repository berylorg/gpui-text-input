use gpui::StreamingLayoutBinding;

use crate::{PageRequestKey, RangeBinding};

use super::*;

impl ExactGeometryOwner {
    /// Reports the exact peak owner bytes required to replace layout inputs.
    pub fn set_layout_required_bytes(
        &self,
        layout: &StreamingLayoutBinding,
        style: &StreamingGeometryStyle,
    ) -> Result<usize, ExactGeometryError> {
        validation::validate_inputs(layout, style)?;
        let binding = self
            .inputs
            .as_deref()
            .ok_or(ExactGeometryError::Disposed)?
            .binding;
        let candidate = OwnerInputs {
            binding,
            layout: layout.clone(),
            style: style.clone(),
        };
        Ok(accounting::counts_with_input_candidate(self, &candidate).total_bytes())
    }

    /// Reports the exact peak semantic records required to replace layout inputs.
    pub fn set_layout_required_items(
        &self,
        layout: &StreamingLayoutBinding,
        style: &StreamingGeometryStyle,
    ) -> Result<usize, ExactGeometryError> {
        validation::validate_inputs(layout, style)?;
        let binding = self
            .inputs
            .as_deref()
            .ok_or(ExactGeometryError::Disposed)?
            .binding;
        let candidate = OwnerInputs {
            binding,
            layout: layout.clone(),
            style: style.clone(),
        };
        Ok(accounting::counts_with_input_candidate(self, &candidate).total_items())
    }

    pub fn cancel(
        &mut self,
        key: GeometryJobKey,
    ) -> Result<ExactGeometryRelease, ExactGeometryError> {
        if self
            .active
            .as_deref()
            .is_some_and(|active| active.key == key)
        {
            let active = self.active.take().expect("active key checked");
            return Ok(ExactGeometryRelease {
                jobs: vec![key],
                pages: active.pending.as_deref().copied().into_iter().collect(),
                counts: accounting::active_counts(&active),
            });
        }
        if self
            .desired_target
            .as_deref()
            .is_some_and(|desired| desired.key == key)
        {
            let release =
                owner::desired_release(self.desired_target.take().expect("desired key checked"));
            self.refresh_active_capacity()?;
            return Ok(release);
        }
        Err(ExactGeometryError::ObsoleteJob(key))
    }

    pub fn fail_page(
        &mut self,
        key: GeometryJobKey,
        page: PageRequestKey,
    ) -> Result<ExactGeometryRelease, ExactGeometryError> {
        let active = self
            .active
            .as_deref()
            .ok_or(ExactGeometryError::ObsoleteJob(key))?;
        if active.key != key {
            return Err(ExactGeometryError::ObsoleteJob(key));
        }
        if active.pending.as_deref().copied() != Some(page) {
            return Err(ExactGeometryError::WrongPage(page));
        }
        self.cancel(key)
    }

    pub fn rebind(
        &mut self,
        binding: RangeBinding,
    ) -> Result<ExactGeometryRelease, ExactGeometryError> {
        let next_epoch = self.next_epoch()?;
        let inputs = self.inputs.as_deref().ok_or(ExactGeometryError::Disposed)?;
        let candidate = Box::new(OwnerInputs {
            binding,
            layout: inputs.layout.clone(),
            style: inputs.style.clone(),
        });
        self.admit_input_candidate(&candidate)?;
        let release = self.release_all(true);
        self.key = GeometryKey::new(binding.binding(), binding.revision(), next_epoch);
        self.inputs = Some(candidate);
        Ok(release)
    }

    pub fn set_layout(
        &mut self,
        layout: StreamingLayoutBinding,
        style: StreamingGeometryStyle,
    ) -> Result<ExactGeometryRelease, ExactGeometryError> {
        validation::validate_inputs(&layout, &style)?;
        let next_epoch = self.next_epoch()?;
        let binding = self
            .inputs
            .as_deref()
            .ok_or(ExactGeometryError::Disposed)?
            .binding;
        let candidate = Box::new(OwnerInputs {
            binding,
            layout,
            style,
        });
        self.admit_input_candidate(&candidate)?;
        let release = self.release_all(true);
        self.key = GeometryKey::new(binding.binding(), binding.revision(), next_epoch);
        self.inputs = Some(candidate);
        Ok(release)
    }

    pub fn dispose(&mut self) -> ExactGeometryRelease {
        self.release_all(true)
    }

    pub fn counts(&self) -> ExactGeometryCounts {
        accounting::owner_counts(self)
    }

    /// Moves the current exact target publication to its final surface owner.
    ///
    /// The shaped graph is not cloned. Its charge therefore transfers to the caller and stops
    /// contributing to this coordinator's counts.
    pub fn take_target(&mut self) -> Option<BlockTargetPublication> {
        self.target.take().map(|target| *target)
    }

    fn admit_input_candidate(&mut self, inputs: &OwnerInputs) -> Result<(), ExactGeometryError> {
        let counts = accounting::counts_with_input_candidate(self, inputs);
        let retained = counts.total_bytes();
        let retained_items = counts.total_items();
        if retained > self.limits.max_retained_bytes
            || retained_items > self.limits.max_retained_items
        {
            Err(ExactGeometryError::CapacityExceeded)
        } else {
            self.high_water_bytes = self.high_water_bytes.max(retained);
            self.high_water_items = self.high_water_items.max(retained_items);
            Ok(())
        }
    }

    fn release_all(&mut self, release_inputs: bool) -> ExactGeometryRelease {
        let mut counts = self.counts();
        counts.owner_bytes = 0;
        counts.owner_items = 0;
        let mut jobs = Vec::new();
        let mut pages = Vec::new();
        if let Some(active) = self.active.take() {
            jobs.push(active.key);
            pages.extend(active.pending.as_deref().copied());
        }
        if let Some(desired) = self.desired_target.take() {
            jobs.push(desired.key);
        }
        if let Some(index) = self.index.take() {
            jobs.push(index.key);
        }
        if let Some(target) = self.target.take() {
            jobs.push(target.key);
        }
        if release_inputs {
            self.inputs = None;
        }
        jobs.sort();
        jobs.dedup();
        ExactGeometryRelease {
            jobs,
            pages,
            counts,
        }
    }

    fn next_epoch(&self) -> Result<LayoutEpoch, ExactGeometryError> {
        let next = self
            .key
            .epoch()
            .get()
            .checked_add(1)
            .ok_or(ExactGeometryError::EpochExhausted)?;
        Ok(LayoutEpoch::new(next))
    }
}

impl Drop for ExactGeometryOwner {
    fn drop(&mut self) {
        let _ = self.release_all(true);
    }
}
