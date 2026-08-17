use gpui::StreamingLayoutBinding;

use crate::{PageRequestKey, RangeBinding};

use super::*;

impl ExactGeometryOwner {
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
            presentation_generation: self.key.presentation_generation(),
            layout: layout.clone(),
            style: style.clone(),
        };
        Ok(accounting::counts_with_input_candidate(self, &candidate).total_bytes())
    }

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
            presentation_generation: self.key.presentation_generation(),
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
            let mut pages = Vec::new();
            let mut object_pages = Vec::new();
            match active.pending.as_deref().copied() {
                Some(PendingInput::Text(page)) => pages.push(page),
                Some(PendingInput::Object(page)) => object_pages.push(page),
                None => {}
            }
            return Ok(ExactGeometryRelease {
                jobs: vec![key],
                pages,
                object_pages,
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
        if active.pending.as_deref().copied() != Some(PendingInput::Text(page)) {
            return Err(ExactGeometryError::WrongPage(page));
        }
        self.cancel(key)
    }

    pub fn fail_object_page(
        &mut self,
        key: GeometryJobKey,
        page: crate::ObjectRequestKey,
    ) -> Result<ExactGeometryRelease, ExactGeometryError> {
        let active = self
            .active
            .as_deref()
            .ok_or(ExactGeometryError::ObsoleteJob(key))?;
        if active.key != key {
            return Err(ExactGeometryError::ObsoleteJob(key));
        }
        if active.pending.as_deref().copied() != Some(PendingInput::Object(page)) {
            return Err(ExactGeometryError::WrongObjectPage(page));
        }
        self.cancel(key)
    }

    pub fn rebind(
        &mut self,
        binding: RangeBinding,
        presentation_generation: crate::PresentationGeneration,
    ) -> Result<ExactGeometryRelease, ExactGeometryError> {
        let next_epoch = self.next_epoch()?;
        let inputs = self.inputs.as_deref().ok_or(ExactGeometryError::Disposed)?;
        let candidate = Box::new(OwnerInputs {
            binding,
            presentation_generation,
            layout: inputs.layout.clone(),
            style: inputs.style.clone(),
        });
        self.admit_input_candidate(&candidate)?;
        let release = self.release_all(true);
        self.key = GeometryKey::new(
            binding.binding(),
            binding.revision(),
            presentation_generation,
            next_epoch,
        );
        self.inputs = Some(candidate);
        self.highest_object_request = None;
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
            presentation_generation: self.key.presentation_generation(),
            layout,
            style,
        });
        self.admit_input_candidate(&candidate)?;
        let release = self.release_all(true);
        self.key = GeometryKey::new(
            binding.binding(),
            binding.revision(),
            self.key.presentation_generation(),
            next_epoch,
        );
        self.inputs = Some(candidate);
        Ok(release)
    }

    pub fn set_presentation_generation(
        &mut self,
        presentation_generation: crate::PresentationGeneration,
    ) -> Result<ExactGeometryRelease, ExactGeometryError> {
        let inputs = self.inputs.as_deref().ok_or(ExactGeometryError::Disposed)?;
        if self.key.presentation_generation() == presentation_generation {
            return Ok(ExactGeometryRelease::default());
        }
        let candidate = Box::new(OwnerInputs {
            binding: inputs.binding,
            presentation_generation,
            layout: inputs.layout.clone(),
            style: inputs.style.clone(),
        });
        self.admit_input_candidate(&candidate)?;
        let release = self.release_all(true);
        self.key = GeometryKey::new(
            candidate.binding.binding(),
            candidate.binding.revision(),
            presentation_generation,
            self.key.epoch(),
        );
        self.inputs = Some(candidate);
        self.highest_object_request = None;
        Ok(release)
    }

    pub fn dispose(&mut self) -> ExactGeometryRelease {
        self.release_all(true)
    }

    pub fn counts(&self) -> ExactGeometryCounts {
        accounting::owner_counts(self)
    }

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
        let mut object_pages = Vec::new();
        if let Some(active) = self.active.take() {
            jobs.push(active.key);
            match active.pending.as_deref().copied() {
                Some(PendingInput::Text(page)) => pages.push(page),
                Some(PendingInput::Object(page)) => object_pages.push(page),
                None => {}
            }
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
            object_pages,
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
