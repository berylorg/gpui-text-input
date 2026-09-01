use gpui::StreamingLayoutBinding;

use crate::{
    ByteOffset, ObjectDemandEnvelope, ObjectDirection, ObjectPurpose, ObjectRequest,
    ObjectRequestId, PageDirection, PagePurpose, PageRequest, PageRequestId,
    PresentationGeneration, RangeBinding,
};

use super::*;

impl ExactGeometryOwner {
    pub(crate) fn presentation_overlap_bytes<'a>(
        &self,
        pages: impl Iterator<Item = &'a crate::ObjectPage> + Clone,
    ) -> Option<usize> {
        accounting::presentation_overlap_bytes(self, pages)
    }

    pub fn initial_required_charge(
        layout: &StreamingLayoutBinding,
        style: &StreamingGeometryStyle,
    ) -> Result<(usize, usize), ExactGeometryError> {
        validation::validate_inputs(layout, style)?;
        let counts = accounting::initial_owner_counts(layout, style);
        Ok((counts.total_bytes(), counts.total_items()))
    }

    pub(crate) fn pending_layout_style_charge(
        layout: &StreamingLayoutBinding,
        style: &StreamingGeometryStyle,
    ) -> (usize, usize) {
        accounting::layout_style_counts(layout, style)
    }

    pub fn new(
        binding: RangeBinding,
        presentation_generation: PresentationGeneration,
        layout: StreamingLayoutBinding,
        style: StreamingGeometryStyle,
        limits: ExactGeometryLimits,
    ) -> Result<Self, ExactGeometryError> {
        validation::validate_inputs(&layout, &style)?;
        let key = GeometryKey::new(
            binding.binding(),
            binding.revision(),
            presentation_generation,
            LayoutEpoch::new(1),
        );
        let mut owner = Self {
            inputs: Some(Box::new(OwnerInputs {
                binding,
                presentation_generation,
                layout,
                style,
            })),
            limits,
            key,
            highest_job: None,
            highest_request: None,
            highest_object_request: None,
            active: None,
            desired_target: None,
            index: None,
            target: None,
            high_water_bytes: 0,
            high_water_items: 0,
        };
        accounting::ensure_owner(&owner)?;
        owner.observe_current();
        Ok(owner)
    }

    pub const fn key(&self) -> GeometryKey {
        self.key
    }

    pub fn index(&self) -> Option<&ExactGeometryIndex> {
        self.index.as_deref()
    }

    pub fn target(&self) -> Option<&BlockTargetPublication> {
        self.target.as_deref()
    }

    pub fn desired_target_key(&self) -> Option<GeometryJobKey> {
        self.desired_target.as_ref().map(|desired| desired.key)
    }

    pub fn active_text_page(&self, key: GeometryJobKey) -> Option<PageId> {
        self.active
            .as_deref()
            .filter(|active| active.key == key)
            .and_then(|active| active.text_page.map(|page| page.id))
    }

    pub const fn retained_high_water_bytes(&self) -> usize {
        self.high_water_bytes
    }

    pub const fn retained_high_water_items(&self) -> usize {
        self.high_water_items
    }

    pub fn estimate(&self) -> Option<StreamingGeometryEstimate> {
        let active = self.active.as_deref()?;
        matches!(active.kind, ActiveKind::Index).then_some(StreamingGeometryEstimate {
            scanned_source: active.scanner.continuation.next_position.try_into().ok()?,
            visual_lines_lower_bound: active.scanner.continuation.visual_lines,
            content_height_lower_bound: active.scanner.continuation.block_offset,
        })
    }

    pub fn start_index(
        &mut self,
        id: GeometryJobId,
    ) -> Result<ExactGeometryStart, ExactGeometryError> {
        self.admit_job_id(id)?;
        if self.active.is_some() {
            return Err(ExactGeometryError::Busy);
        }
        let inputs = self.inputs()?;
        let source_len = usize::try_from(inputs.binding.extent().byte_len())
            .map_err(|_| ExactGeometryError::SourceContract)?;
        let key = GeometryJobKey::new(self.key, id);
        let fixed = accounting::fixed_bytes_without_active(self);
        let retained_capacity = self
            .limits
            .max_retained_bytes
            .checked_sub(fixed)
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        let mut active = ActiveJob {
            key,
            kind: ActiveKind::Index,
            page_use: ActivePageUse::Traverse {
                anchor: ByteOffset::new(0),
            },
            pending: None,
            text_page: None,
            window_identity: None,
            retained_capacity,
            scanner: Scanner::origin(&inputs.layout, source_len),
        };
        accounting::ensure_active(&mut active)?;
        let start_items = accounting::fixed_counts_without_active(self)
            .total_items()
            .saturating_add(accounting::active_counts(&active).total_items());
        if start_items > self.limits.max_retained_items {
            return Err(ExactGeometryError::CapacityExceeded);
        }
        self.active = Some(Box::new(active));
        self.highest_job = Some(id);
        self.observe_current();
        Ok(self.start_result(key, ExactGeometryProgress::Scanning))
    }

    pub fn request_page(
        &mut self,
        key: GeometryJobKey,
        id: PageRequestId,
    ) -> Result<PageRequest, ExactGeometryError> {
        let expected = self.preview_page_request(key, id)?;
        let active = self
            .active
            .as_deref_mut()
            .ok_or(ExactGeometryError::ObsoleteJob(key))?;
        active.pending = Some(Box::new(PendingInput::Text(expected.key())));
        self.highest_request = Some(id);
        self.observe_current();
        Ok(expected)
    }

    pub(crate) fn preview_page_request(
        &self,
        key: GeometryJobKey,
        id: PageRequestId,
    ) -> Result<PageRequest, ExactGeometryError> {
        let binding = self.inputs()?.binding;
        let retained_item_capacity = self
            .limits
            .max_retained_items
            .checked_sub(accounting::fixed_counts_without_active(self).total_items())
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        let active = self
            .active
            .as_deref()
            .ok_or(ExactGeometryError::ObsoleteJob(key))?;
        if active.key != key {
            return Err(ExactGeometryError::ObsoleteJob(key));
        }
        if active.pending.is_some() || active.text_page.is_some() {
            return Err(ExactGeometryError::PageAlreadyPending);
        }
        if self.highest_request.is_some_and(|highest| id <= highest) {
            return Err(ExactGeometryError::IdNotMonotonic);
        }
        let purpose = match active.kind {
            ActiveKind::Index => PagePurpose::GeometryIndex,
            ActiveKind::Target { .. } => PagePurpose::GeometryTarget,
        };
        let (anchor, direction) = match active.page_use {
            ActivePageUse::Traverse { anchor } => (anchor, PageDirection::Forward),
            ActivePageUse::Context { required_end, .. } => (required_end, PageDirection::Backward),
        };
        let page_key = PageRequestKey::adjacent(
            id,
            binding.binding(),
            binding.revision(),
            purpose,
            anchor,
            direction,
            self.limits.max_page_bytes,
        )
        .map_err(|_| ExactGeometryError::InvalidLimits)?;
        let active_counts = accounting::active_counts(active);
        if active_counts
            .total_bytes()
            .checked_add(std::mem::size_of::<PageRequestKey>())
            .is_none_or(|bytes| bytes > active.retained_capacity)
            || active_counts
                .total_items()
                .checked_add(1)
                .is_none_or(|items| items > retained_item_capacity)
        {
            return Err(ExactGeometryError::CapacityExceeded);
        }
        Ok(PageRequest::new(page_key))
    }

    pub fn request_object_page(
        &mut self,
        key: GeometryJobKey,
        id: ObjectRequestId,
        max_objects: usize,
        max_retained_bytes: usize,
    ) -> Result<ObjectRequest, ExactGeometryError> {
        let expected =
            self.preview_object_page_request(key, id, max_objects, max_retained_bytes)?;
        let active = self
            .active
            .as_deref_mut()
            .ok_or(ExactGeometryError::ObsoleteJob(key))?;
        active.pending = Some(Box::new(PendingInput::Object(expected.key())));
        self.highest_object_request = Some(id);
        self.observe_current();
        Ok(expected)
    }

    pub(crate) fn preview_object_page_request(
        &self,
        key: GeometryJobKey,
        id: ObjectRequestId,
        max_objects: usize,
        max_retained_bytes: usize,
    ) -> Result<ObjectRequest, ExactGeometryError> {
        let (binding, presentation_generation) = {
            let inputs = self.inputs()?;
            (inputs.binding, inputs.presentation_generation)
        };
        let retained_item_capacity = self
            .limits
            .max_retained_items
            .checked_sub(accounting::fixed_counts_without_active(self).total_items())
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        let active = self
            .active
            .as_deref()
            .ok_or(ExactGeometryError::ObsoleteJob(key))?;
        if active.key != key {
            return Err(ExactGeometryError::ObsoleteJob(key));
        }
        if active.pending.is_some() {
            return Err(ExactGeometryError::PageAlreadyPending);
        }
        if self
            .highest_object_request
            .is_some_and(|highest| id <= highest)
        {
            return Err(ExactGeometryError::IdNotMonotonic);
        }
        let page = active.text_page.ok_or(ExactGeometryError::WrongInputKind)?;
        let purpose = match active.kind {
            ActiveKind::Index => ObjectPurpose::GeometryIndex,
            ActiveKind::Target { .. } => ObjectPurpose::GeometryTarget,
        };
        let cursor = active
            .scanner
            .object_cursor
            .filter(|cursor| page.range.contains_offset(cursor.anchor()));
        let demand = ObjectDemandEnvelope::range(
            page.range,
            cursor,
            ObjectDirection::Forward,
            max_objects,
            max_retained_bytes,
        )
        .map_err(|_| ExactGeometryError::SourceContract)?;
        let request_key = crate::ObjectRequestKey::new(
            id,
            binding.binding(),
            binding.revision(),
            presentation_generation,
            purpose,
            demand,
        )
        .map_err(|_| ExactGeometryError::SourceContract)?;
        let active_counts = accounting::active_counts(active);
        if active_counts
            .total_bytes()
            .checked_add(std::mem::size_of::<crate::ObjectRequestKey>())
            .is_none_or(|bytes| bytes > active.retained_capacity)
            || active_counts
                .total_items()
                .checked_add(1)
                .is_none_or(|items| items > retained_item_capacity)
        {
            return Err(ExactGeometryError::CapacityExceeded);
        }
        Ok(ObjectRequest::new(request_key))
    }

    pub(super) fn inputs(&self) -> Result<&OwnerInputs, ExactGeometryError> {
        self.inputs.as_deref().ok_or(ExactGeometryError::Disposed)
    }

    pub(super) fn admit_job_id(&self, id: GeometryJobId) -> Result<(), ExactGeometryError> {
        self.inputs()?;
        if self.highest_job.is_some_and(|highest| id <= highest) {
            return Err(ExactGeometryError::IdNotMonotonic);
        }
        Ok(())
    }

    pub(super) fn start_result(
        &self,
        key: GeometryJobKey,
        progress: ExactGeometryProgress,
    ) -> ExactGeometryStart {
        ExactGeometryStart {
            key,
            progress,
            release: ExactGeometryRelease::default(),
            admission_required_bytes: self.counts().total_bytes(),
            admission_required_items: self.counts().total_items(),
        }
    }

    pub(super) fn refresh_active_capacity(&mut self) -> Result<(), ExactGeometryError> {
        let fixed = accounting::fixed_bytes_without_active(self);
        let capacity = self
            .limits
            .max_retained_bytes
            .checked_sub(fixed)
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        if let Some(active) = self.active.as_deref_mut() {
            active.retained_capacity = capacity;
            accounting::ensure_active(active)?;
        }
        accounting::ensure_owner(self)?;
        self.observe_current();
        Ok(())
    }

    pub(super) fn observe_current(&mut self) {
        let counts = accounting::owner_counts(self);
        self.high_water_bytes = self.high_water_bytes.max(counts.total_bytes());
        self.high_water_items = self.high_water_items.max(counts.total_items());
    }
}

pub(super) fn desired_release(desired: Box<DesiredTarget>) -> ExactGeometryRelease {
    ExactGeometryRelease {
        jobs: vec![desired.key],
        counts: ExactGeometryCounts {
            desired_target_items: 1,
            desired_target_bytes: std::mem::size_of::<DesiredTarget>(),
            ..Default::default()
        },
        ..Default::default()
    }
}
