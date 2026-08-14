use std::sync::Arc;

use gpui::StreamingLayoutBinding;

use crate::{ByteOffset, PageDirection, PagePurpose, PageRequest, PageRequestId, RangeBinding};

use super::*;

impl ExactGeometryOwner {
    pub fn new(
        binding: RangeBinding,
        layout: StreamingLayoutBinding,
        style: StreamingGeometryStyle,
        limits: ExactGeometryLimits,
    ) -> Result<Self, ExactGeometryError> {
        validation::validate_inputs(&layout, &style)?;
        let key = GeometryKey::new(binding.binding(), binding.revision(), LayoutEpoch::new(1));
        let mut owner = Self {
            inputs: Some(Box::new(OwnerInputs {
                binding,
                layout,
                style,
            })),
            limits,
            key,
            highest_job: None,
            highest_request: None,
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

    pub const fn retained_high_water_bytes(&self) -> usize {
        self.high_water_bytes
    }

    /// Highest exact semantic-record residency observed by this owner.
    pub const fn retained_high_water_items(&self) -> usize {
        self.high_water_items
    }

    pub fn estimate(&self) -> Option<StreamingGeometryEstimate> {
        let active = self.active.as_deref()?;
        matches!(active.kind, ActiveKind::Index).then_some(StreamingGeometryEstimate {
            scanned_source: ByteOffset::new(active.scanner.continuation.next_logical_offset),
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
        if source_len == 0 {
            let scanner = Scanner::origin(&inputs.layout, 0);
            let origin = scanner
                .checkpoints
                .front()
                .expect("origin checkpoint")
                .clone();
            let mut terminal = origin.clone();
            terminal.terminal = true;
            let candidate = ExactGeometryIndex {
                key,
                checkpoints: Arc::from([origin, terminal]),
                aggregate: ExactGeometryAggregate {
                    visual_lines: 0,
                    content_height: gpui::Pixels::ZERO,
                },
            };
            let counts = accounting::counts_with_index_candidate(self, &candidate);
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
            let prior = self.index.replace(Box::new(candidate));
            self.highest_job = Some(id);
            self.observe_current();
            return Ok(ExactGeometryStart {
                key,
                progress: ExactGeometryProgress::IndexComplete,
                release: prior.map_or_else(ExactGeometryRelease::default, admission::index_release),
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
            kind: ActiveKind::Index,
            page_use: ActivePageUse::Traverse {
                anchor: ByteOffset::new(0),
            },
            pending: None,
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
        let binding = self.inputs()?.binding;
        let active = self
            .active
            .as_deref_mut()
            .ok_or(ExactGeometryError::ObsoleteJob(key))?;
        if active.key != key {
            return Err(ExactGeometryError::ObsoleteJob(key));
        }
        if active.pending.is_some() {
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
        active.pending = Some(Box::new(page_key));
        if let Err(error) = accounting::ensure_active(active) {
            active.pending = None;
            return Err(error);
        }
        self.highest_request = Some(id);
        self.observe_current();
        Ok(PageRequest::new(page_key))
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
