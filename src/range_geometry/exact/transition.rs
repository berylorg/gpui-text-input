use std::{mem::size_of, sync::Arc};

use gpui::StreamingLayoutBinding;

use crate::{
    GeometryJobId, GeometryJobKey, GeometryKey, LayoutEpoch, PageDirection, PagePurpose,
    PageRequest, PageRequestId, PageRequestKey, PresentationGeneration, RangeBinding,
    SourcePosition,
};

use super::{
    ActiveJob, ActiveKind, ActivePageUse, BlockTarget, BlockTargetPublication, DesiredTarget,
    ExactGeometryCheckpoint, ExactGeometryError, ExactGeometryIndex, ExactGeometryOwner,
    ExactGeometryProgress, ExactGeometryRelease, ExactGeometryStart, OwnerInputs, PendingInput,
    Scanner, StreamingGeometryStyle, accounting, validation,
};

#[derive(Debug)]
pub(crate) struct PreparedGeometryTransition {
    key: GeometryKey,
    inputs: Option<Box<OwnerInputs>>,
    state: PreparedGeometryState,
    release: ExactGeometryRelease,
    highest_job: GeometryJobId,
    highest_request: Option<PageRequestId>,
    reset_object_request: bool,
    admission_required_bytes: usize,
    admission_required_items: usize,
}

#[derive(Debug)]
enum PreparedGeometryState {
    Index(Box<ActiveJob>),
    Desired(Box<DesiredTarget>),
    Target(Box<ActiveJob>),
    Complete(Box<BlockTargetPublication>),
}

impl PreparedGeometryTransition {
    pub(crate) const fn key(&self) -> GeometryJobKey {
        match &self.state {
            PreparedGeometryState::Index(active) | PreparedGeometryState::Target(active) => {
                active.key
            }
            PreparedGeometryState::Desired(desired) => desired.key,
            PreparedGeometryState::Complete(target) => target.key,
        }
    }

    pub(crate) fn page_request(&self) -> Option<PageRequest> {
        match &self.state {
            PreparedGeometryState::Index(active) | PreparedGeometryState::Target(active) => {
                match active.pending.as_deref().copied() {
                    Some(PendingInput::Text(key)) => Some(PageRequest::new(key)),
                    _ => None,
                }
            }
            PreparedGeometryState::Desired(_) | PreparedGeometryState::Complete(_) => None,
        }
    }

    pub(crate) fn release(&self) -> &ExactGeometryRelease {
        &self.release
    }

    pub(crate) fn terminal_target(&self) -> Option<&BlockTargetPublication> {
        match &self.state {
            PreparedGeometryState::Complete(target) => Some(target),
            _ => None,
        }
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        let state = match &self.state {
            PreparedGeometryState::Index(active) | PreparedGeometryState::Target(active) => {
                accounting::active_counts(active)
            }
            PreparedGeometryState::Desired(_) => accounting::desired_counts(),
            PreparedGeometryState::Complete(target) => accounting::target_counts(target),
        };
        let inputs = self
            .inputs
            .as_deref()
            .map_or(Default::default(), accounting::input_counts);
        state
            .total_bytes()
            .saturating_add(inputs.total_bytes())
            .saturating_add(
                self.release
                    .jobs
                    .capacity()
                    .saturating_mul(size_of::<GeometryJobKey>()),
            )
            .saturating_add(
                self.release
                    .pages
                    .capacity()
                    .saturating_mul(size_of::<PageRequestKey>()),
            )
            .saturating_add(
                self.release
                    .object_pages
                    .capacity()
                    .saturating_mul(size_of::<crate::ObjectRequestKey>()),
            )
    }

    pub(crate) fn retained_items(&self) -> usize {
        let state = match &self.state {
            PreparedGeometryState::Index(active) | PreparedGeometryState::Target(active) => {
                accounting::active_counts(active)
            }
            PreparedGeometryState::Desired(_) => accounting::desired_counts(),
            PreparedGeometryState::Complete(target) => accounting::target_counts(target),
        };
        let inputs = self
            .inputs
            .as_deref()
            .map_or(Default::default(), accounting::input_counts);
        state
            .total_items()
            .saturating_add(inputs.total_items())
            .saturating_add(self.release.jobs.capacity())
            .saturating_add(self.release.pages.capacity())
            .saturating_add(self.release.object_pages.capacity())
    }

    pub(crate) const fn admission_required_bytes(&self) -> usize {
        self.admission_required_bytes
    }

    pub(crate) const fn admission_required_items(&self) -> usize {
        self.admission_required_items
    }
}

impl ExactGeometryOwner {
    pub(crate) fn prepare_start_index(
        &self,
        job_id: GeometryJobId,
        request_id: PageRequestId,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        self.admit_job_id(job_id)?;
        if self.active.is_some() {
            return Err(ExactGeometryError::Busy);
        }
        let inputs = self.inputs()?;
        let key = GeometryJobKey::new(self.key, job_id);
        let active = self.prepare_index_active(inputs, key, request_id, self.fixed_bytes())?;
        self.finish_prepared(
            self.key,
            None,
            PreparedGeometryState::Index(active),
            ExactGeometryRelease::default(),
            job_id,
            Some(request_id),
            false,
        )
    }

    pub(crate) fn prepare_layout_and_index(
        &self,
        layout: StreamingLayoutBinding,
        style: StreamingGeometryStyle,
        job_id: GeometryJobId,
        request_id: PageRequestId,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        validation::validate_inputs(&layout, &style)?;
        let binding = self.inputs()?.binding;
        let epoch = self.next_transition_epoch()?;
        let inputs = Box::new(OwnerInputs {
            binding,
            presentation_generation: self.key.presentation_generation(),
            layout,
            style,
        });
        let key = GeometryKey::new(
            binding.binding(),
            binding.revision(),
            self.key.presentation_generation(),
            epoch,
        );
        self.prepare_replacement_index(inputs, key, job_id, request_id, false)
    }

    pub(crate) fn prepare_presentation_and_index(
        &self,
        presentation_generation: PresentationGeneration,
        job_id: GeometryJobId,
        request_id: PageRequestId,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        let current = self.inputs()?;
        let inputs = Box::new(OwnerInputs {
            binding: current.binding,
            presentation_generation,
            layout: current.layout.clone(),
            style: current.style.clone(),
        });
        let key = GeometryKey::new(
            current.binding.binding(),
            current.binding.revision(),
            presentation_generation,
            self.key.epoch(),
        );
        self.prepare_replacement_index(inputs, key, job_id, request_id, true)
    }

    pub(crate) fn prepare_rebind_and_index(
        &self,
        binding: RangeBinding,
        presentation_generation: PresentationGeneration,
        job_id: GeometryJobId,
        request_id: PageRequestId,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        let current = self.inputs()?;
        let epoch = self.next_transition_epoch()?;
        let inputs = Box::new(OwnerInputs {
            binding,
            presentation_generation,
            layout: current.layout.clone(),
            style: current.style.clone(),
        });
        let key = GeometryKey::new(
            binding.binding(),
            binding.revision(),
            presentation_generation,
            epoch,
        );
        self.prepare_replacement_index(inputs, key, job_id, request_id, true)
    }

    pub(crate) fn prepare_layout_and_origin_target(
        &self,
        layout: StreamingLayoutBinding,
        style: StreamingGeometryStyle,
        job_id: GeometryJobId,
        request_id: PageRequestId,
        target: BlockTarget,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        validation::validate_inputs(&layout, &style)?;
        let binding = self.inputs()?.binding;
        let epoch = self.next_transition_epoch()?;
        let inputs = Box::new(OwnerInputs {
            binding,
            presentation_generation: self.key.presentation_generation(),
            layout,
            style,
        });
        let key = GeometryKey::new(
            binding.binding(),
            binding.revision(),
            self.key.presentation_generation(),
            epoch,
        );
        self.prepare_replacement_origin_target(inputs, key, job_id, request_id, target, false)
    }

    pub(crate) fn prepare_presentation_and_origin_target(
        &self,
        presentation_generation: PresentationGeneration,
        job_id: GeometryJobId,
        request_id: PageRequestId,
        target: BlockTarget,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        let current = self.inputs()?;
        let inputs = Box::new(OwnerInputs {
            binding: current.binding,
            presentation_generation,
            layout: current.layout.clone(),
            style: current.style.clone(),
        });
        let key = GeometryKey::new(
            current.binding.binding(),
            current.binding.revision(),
            presentation_generation,
            self.key.epoch(),
        );
        self.prepare_replacement_origin_target(inputs, key, job_id, request_id, target, true)
    }

    pub(crate) fn prepare_rebind_and_origin_target(
        &self,
        binding: RangeBinding,
        presentation_generation: PresentationGeneration,
        job_id: GeometryJobId,
        request_id: PageRequestId,
        target: BlockTarget,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        let current = self.inputs()?;
        let epoch = self.next_transition_epoch()?;
        let inputs = Box::new(OwnerInputs {
            binding,
            presentation_generation,
            layout: current.layout.clone(),
            style: current.style.clone(),
        });
        let key = GeometryKey::new(
            binding.binding(),
            binding.revision(),
            presentation_generation,
            epoch,
        );
        self.prepare_replacement_origin_target(inputs, key, job_id, request_id, target, true)
    }

    fn prepare_replacement_origin_target(
        &self,
        inputs: Box<OwnerInputs>,
        key: GeometryKey,
        job_id: GeometryJobId,
        request_id: PageRequestId,
        target: BlockTarget,
        reset_object_request: bool,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        super::target::validate_target(target)?;
        self.admit_transition_job_id(job_id)?;
        self.admit_transition_request_id(request_id)?;
        let scanner = Scanner::origin(
            &inputs.layout,
            usize::try_from(inputs.binding.extent().byte_len())
                .map_err(|_| ExactGeometryError::SourceContract)?,
        );
        let predecessor = super::checkpoint::make_checkpoint(&scanner, &inputs.layout, false)?;
        let fixed = accounting::counts(None, None, None, None, None)
            .total_bytes()
            .saturating_add(accounting::input_counts(&inputs).total_bytes());
        let active = self.prepare_target_active_for_inputs(
            &inputs,
            GeometryJobKey::new(key, job_id),
            target,
            predecessor.source,
            predecessor.clone(),
            None,
            Scanner::from_checkpoint(&predecessor),
            request_id,
            fixed,
        )?;
        self.finish_prepared(
            key,
            Some(inputs),
            PreparedGeometryState::Target(active),
            self.preview_release_all(),
            job_id,
            Some(request_id),
            reset_object_request,
        )
    }

    fn prepare_replacement_index(
        &self,
        inputs: Box<OwnerInputs>,
        key: GeometryKey,
        job_id: GeometryJobId,
        request_id: PageRequestId,
        reset_object_request: bool,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        self.admit_transition_job_id(job_id)?;
        self.admit_transition_request_id(request_id)?;
        let fixed = accounting::counts(None, None, None, None, None)
            .total_bytes()
            .saturating_add(accounting::input_counts(&inputs).total_bytes());
        let job_key = GeometryJobKey::new(key, job_id);
        let active = self.prepare_index_active(&inputs, job_key, request_id, fixed)?;
        self.finish_prepared(
            key,
            Some(inputs),
            PreparedGeometryState::Index(active),
            self.preview_release_all(),
            job_id,
            Some(request_id),
            reset_object_request,
        )
    }

    pub(crate) fn prepare_target_replacement(
        &self,
        job_id: GeometryJobId,
        request_id: PageRequestId,
        target: BlockTarget,
        anchor: Option<SourcePosition>,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        super::target::validate_target(target)?;
        let inputs = self.inputs()?;
        if anchor
            .is_some_and(|anchor| anchor.byte_offset.get() > inputs.binding.extent().byte_len())
        {
            return Err(ExactGeometryError::SourceContract);
        }
        self.admit_transition_job_id(job_id)?;
        let key = GeometryJobKey::new(self.key, job_id);
        let release = self.preview_target_replacement_release();

        if self.index.is_none()
            || self
                .active
                .as_deref()
                .is_some_and(|active| matches!(active.kind, ActiveKind::Index))
        {
            return self.finish_prepared(
                self.key,
                None,
                PreparedGeometryState::Desired(Box::new(DesiredTarget {
                    key,
                    target,
                    anchor,
                })),
                release,
                job_id,
                None,
                false,
            );
        }

        let index = self
            .index
            .as_deref()
            .ok_or(ExactGeometryError::IndexIncomplete)?;
        self.prepare_target_replacement_from_index_inner(
            index, key, job_id, request_id, target, anchor, release,
        )
    }

    pub(crate) fn prepare_local_target_replacement(
        &self,
        job_id: GeometryJobId,
        request_id: PageRequestId,
        target: BlockTarget,
        anchor: Option<SourcePosition>,
        checkpoint: Option<&ExactGeometryCheckpoint>,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        super::target::validate_target(target)?;
        let inputs = self.inputs()?;
        if anchor
            .is_some_and(|anchor| anchor.byte_offset.get() > inputs.binding.extent().byte_len())
        {
            return Err(ExactGeometryError::SourceContract);
        }
        self.admit_transition_job_id(job_id)?;
        let key = GeometryJobKey::new(self.key, job_id);
        let predecessor = if let Some(checkpoint) = checkpoint {
            checkpoint.clone()
        } else {
            super::checkpoint::make_checkpoint(
                &Scanner::origin(
                    &inputs.layout,
                    usize::try_from(inputs.binding.extent().byte_len())
                        .map_err(|_| ExactGeometryError::SourceContract)?,
                ),
                &inputs.layout,
                false,
            )?
        };
        self.prepare_target_replacement_from_checkpoint_inner(
            key,
            job_id,
            request_id,
            target,
            anchor,
            predecessor,
            self.preview_target_replacement_release(),
        )
    }

    pub(crate) fn prepare_target_replacement_from_index(
        &self,
        index: &ExactGeometryIndex,
        job_id: GeometryJobId,
        request_id: PageRequestId,
        target: BlockTarget,
        anchor: Option<SourcePosition>,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        super::target::validate_target(target)?;
        let inputs = self.inputs()?;
        if anchor
            .is_some_and(|anchor| anchor.byte_offset.get() > inputs.binding.extent().byte_len())
        {
            return Err(ExactGeometryError::SourceContract);
        }
        self.admit_transition_job_id(job_id)?;
        let key = GeometryJobKey::new(self.key, job_id);
        self.prepare_target_replacement_from_index_inner(
            index,
            key,
            job_id,
            request_id,
            target,
            anchor,
            self.preview_target_replacement_release(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_target_replacement_from_index_inner(
        &self,
        index: &ExactGeometryIndex,
        key: GeometryJobKey,
        job_id: GeometryJobId,
        request_id: PageRequestId,
        target: BlockTarget,
        anchor: Option<SourcePosition>,
        release: ExactGeometryRelease,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        let inputs = self.inputs()?;
        let source_len = inputs.binding.extent().byte_len();
        let predecessor = if let Some(anchor) = anchor {
            let include_preceding_object = matches!(
                anchor.gap,
                crate::InlineObjectGap::Between { .. } | crate::InlineObjectGap::After(_)
            );
            index
                .checkpoints
                .iter()
                .rev()
                .find(|checkpoint| {
                    checkpoint
                        .source
                        .compare_in_revision(anchor)
                        .is_some_and(|ordering| {
                            ordering.is_lt()
                                || (!include_preceding_object
                                    && ordering.is_eq()
                                    && (source_len == 0 || anchor.byte_offset.get() < source_len))
                        })
                })
                .ok_or(ExactGeometryError::SourceContract)?
                .clone()
        } else {
            index
                .checkpoints
                .iter()
                .rev()
                .find(|checkpoint| {
                    checkpoint.source.byte_offset.get() == 0
                        || checkpoint.resume_block_offset() <= target.block_offset
                })
                .expect("index has origin")
                .clone()
        };
        self.prepare_target_replacement_from_checkpoint_inner(
            key,
            job_id,
            request_id,
            target,
            anchor,
            predecessor,
            release,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_target_replacement_from_checkpoint_inner(
        &self,
        key: GeometryJobKey,
        job_id: GeometryJobId,
        request_id: PageRequestId,
        mut target: BlockTarget,
        anchor: Option<SourcePosition>,
        predecessor: ExactGeometryCheckpoint,
        release: ExactGeometryRelease,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        let inputs = self.inputs()?;
        if predecessor.input_id != inputs.layout.input_id
            || predecessor.segment_policy_id != inputs.layout.segment_policy_id
            || predecessor.source.byte_offset.get() > inputs.binding.extent().byte_len()
        {
            return Err(ExactGeometryError::SourceContract);
        }
        if anchor.is_some() {
            target.block_offset = predecessor.block_offset;
        }
        if predecessor.source.byte_offset.get() == inputs.binding.extent().byte_len() {
            return self.finish_prepared(
                self.key,
                None,
                PreparedGeometryState::Complete(Box::new(BlockTargetPublication {
                    key,
                    predecessor: predecessor.source,
                    target_source: predecessor.source,
                    source_end: predecessor.source,
                    predecessor_checkpoint: predecessor.clone(),
                    visual_lines_lower_bound: predecessor.visual_lines,
                    content_height_lower_bound: predecessor.resume_block_offset(),
                    fragments: Arc::from([]),
                    charge: Default::default(),
                    item_charge: Default::default(),
                })),
                release,
                job_id,
                None,
                false,
            );
        }

        self.admit_transition_request_id(request_id)?;
        let fixed = accounting::counts(
            self.inputs.as_deref(),
            None,
            None,
            self.index.as_deref(),
            None,
        )
        .total_bytes();
        let active = self.prepare_target_active_for_inputs(
            inputs,
            key,
            target,
            predecessor.source,
            predecessor.clone(),
            anchor,
            Scanner::from_checkpoint(&predecessor),
            request_id,
            fixed,
        )?;
        self.finish_prepared(
            self.key,
            None,
            PreparedGeometryState::Target(active),
            release,
            job_id,
            Some(request_id),
            false,
        )
    }

    fn prepare_index_active(
        &self,
        inputs: &OwnerInputs,
        key: GeometryJobKey,
        request_id: PageRequestId,
        fixed: usize,
    ) -> Result<Box<ActiveJob>, ExactGeometryError> {
        self.admit_transition_request_id(request_id)?;
        let source_len = usize::try_from(inputs.binding.extent().byte_len())
            .map_err(|_| ExactGeometryError::SourceContract)?;
        let retained_capacity = self
            .limits
            .max_retained_bytes
            .checked_sub(fixed)
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        let page_key = PageRequestKey::adjacent(
            request_id,
            inputs.binding.binding(),
            inputs.binding.revision(),
            PagePurpose::GeometryIndex,
            crate::ByteOffset::new(0),
            PageDirection::Forward,
            self.limits.max_page_bytes,
        )
        .map_err(|_| ExactGeometryError::InvalidLimits)?;
        let mut active = Box::new(ActiveJob {
            key,
            kind: ActiveKind::Index,
            page_use: ActivePageUse::Traverse {
                anchor: crate::ByteOffset::new(0),
            },
            pending: Some(Box::new(PendingInput::Text(page_key))),
            text_page: None,
            window_identity: None,
            retained_capacity,
            scanner: Scanner::origin(&inputs.layout, source_len),
        });
        accounting::ensure_active(&mut active)?;
        Ok(active)
    }

    fn prepare_target_active_for_inputs(
        &self,
        inputs: &OwnerInputs,
        key: GeometryJobKey,
        target: BlockTarget,
        predecessor: SourcePosition,
        predecessor_checkpoint: ExactGeometryCheckpoint,
        anchor: Option<SourcePosition>,
        scanner: Scanner,
        request_id: PageRequestId,
        fixed: usize,
    ) -> Result<Box<ActiveJob>, ExactGeometryError> {
        let retained_capacity = self
            .limits
            .max_retained_bytes
            .checked_sub(fixed)
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        let page_key = PageRequestKey::adjacent(
            request_id,
            inputs.binding.binding(),
            inputs.binding.revision(),
            PagePurpose::GeometryTarget,
            predecessor.byte_offset,
            PageDirection::Forward,
            self.limits.max_page_bytes,
        )
        .map_err(|_| ExactGeometryError::InvalidLimits)?;
        let mut active = Box::new(ActiveJob {
            key,
            kind: ActiveKind::Target {
                target,
                predecessor,
                predecessor_checkpoint,
                anchor,
            },
            page_use: ActivePageUse::Traverse {
                anchor: predecessor.byte_offset,
            },
            pending: Some(Box::new(PendingInput::Text(page_key))),
            text_page: None,
            window_identity: None,
            retained_capacity,
            scanner,
        });
        accounting::ensure_active(&mut active)?;
        Ok(active)
    }

    fn finish_prepared(
        &self,
        key: GeometryKey,
        inputs: Option<Box<OwnerInputs>>,
        state: PreparedGeometryState,
        release: ExactGeometryRelease,
        highest_job: GeometryJobId,
        highest_request: Option<PageRequestId>,
        reset_object_request: bool,
    ) -> Result<PreparedGeometryTransition, ExactGeometryError> {
        let component_counts = match &state {
            PreparedGeometryState::Index(active) | PreparedGeometryState::Target(active) => {
                accounting::active_counts(active)
            }
            PreparedGeometryState::Desired(_) => accounting::desired_counts(),
            PreparedGeometryState::Complete(target) => accounting::target_counts(target),
        };
        let mut admission_required_bytes = self
            .counts()
            .total_bytes()
            .saturating_add(component_counts.total_bytes());
        let mut admission_required_items = self
            .counts()
            .total_items()
            .saturating_add(component_counts.total_items());
        if let Some(inputs) = inputs.as_deref() {
            let input_counts = accounting::input_counts(inputs);
            admission_required_bytes =
                admission_required_bytes.saturating_add(input_counts.total_bytes());
            admission_required_items =
                admission_required_items.saturating_add(input_counts.total_items());
        }
        if admission_required_bytes > self.limits.max_retained_bytes
            || admission_required_items > self.limits.max_retained_items
        {
            return Err(ExactGeometryError::CapacityExceeded);
        }
        Ok(PreparedGeometryTransition {
            key,
            inputs,
            state,
            release,
            highest_job,
            highest_request,
            reset_object_request,
            admission_required_bytes,
            admission_required_items,
        })
    }

    fn fixed_bytes(&self) -> usize {
        accounting::fixed_counts_without_active(self).total_bytes()
    }

    fn admit_transition_job_id(&self, id: GeometryJobId) -> Result<(), ExactGeometryError> {
        self.inputs()?;
        if self.highest_job.is_some_and(|highest| id <= highest) {
            Err(ExactGeometryError::IdNotMonotonic)
        } else {
            Ok(())
        }
    }

    fn admit_transition_request_id(&self, id: PageRequestId) -> Result<(), ExactGeometryError> {
        if self.highest_request.is_some_and(|highest| id <= highest) {
            Err(ExactGeometryError::IdNotMonotonic)
        } else {
            Ok(())
        }
    }

    fn next_transition_epoch(&self) -> Result<LayoutEpoch, ExactGeometryError> {
        let next = self
            .key
            .epoch()
            .get()
            .checked_add(1)
            .ok_or(ExactGeometryError::EpochExhausted)?;
        Ok(LayoutEpoch::new(next))
    }

    fn preview_release_all(&self) -> ExactGeometryRelease {
        let mut counts = self.counts();
        counts.owner_bytes = 0;
        counts.owner_items = 0;
        let mut jobs = Vec::new();
        let mut pages = Vec::new();
        let mut object_pages = Vec::new();
        if let Some(active) = self.active.as_deref() {
            jobs.push(active.key);
            match active.pending.as_deref().copied() {
                Some(PendingInput::Text(page)) => pages.push(page),
                Some(PendingInput::Object(page)) => object_pages.push(page),
                None => {}
            }
        }
        if let Some(desired) = self.desired_target.as_deref() {
            jobs.push(desired.key);
        }
        if let Some(index) = self.index.as_deref() {
            jobs.push(index.key);
        }
        if let Some(target) = self.target.as_deref() {
            jobs.push(target.key);
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

    fn preview_target_replacement_release(&self) -> ExactGeometryRelease {
        let mut release = ExactGeometryRelease::default();
        if let Some(active) = self
            .active
            .as_deref()
            .filter(|active| matches!(active.kind, ActiveKind::Target { .. }))
        {
            release.jobs.push(active.key);
            match active.pending.as_deref().copied() {
                Some(PendingInput::Text(page)) => release.pages.push(page),
                Some(PendingInput::Object(page)) => release.object_pages.push(page),
                None => {}
            }
            release.counts = accounting::active_counts(active);
        }
        if let Some(desired) = self.desired_target.as_deref() {
            release.jobs.push(desired.key);
            release.counts.desired_target_items =
                release.counts.desired_target_items.saturating_add(1);
            release.counts.desired_target_bytes = release
                .counts
                .desired_target_bytes
                .saturating_add(size_of::<DesiredTarget>());
        }
        if let Some(target) = self.target.as_deref() {
            release.jobs.push(target.key);
            release.counts =
                accounting::add_counts(release.counts, accounting::target_counts(target));
        }
        release.jobs.sort();
        release.jobs.dedup();
        release
    }

    pub(crate) fn commit_prepared_transition(
        &mut self,
        prepared: PreparedGeometryTransition,
    ) -> ExactGeometryStart {
        let PreparedGeometryTransition {
            key,
            inputs,
            state,
            release,
            highest_job,
            highest_request,
            reset_object_request,
            admission_required_bytes,
            admission_required_items,
        } = prepared;

        if let Some(inputs) = inputs {
            self.inputs = Some(inputs);
            self.active = None;
            self.desired_target = None;
            self.index = None;
            self.target = None;
            self.key = key;
        } else {
            if self
                .active
                .as_deref()
                .is_some_and(|active| matches!(active.kind, ActiveKind::Target { .. }))
            {
                self.active = None;
            }
            self.desired_target = None;
            self.target = None;
        }
        let (job_key, progress) = match state {
            PreparedGeometryState::Index(active) => {
                let key = active.key;
                self.active = Some(active);
                (key, ExactGeometryProgress::Scanning)
            }
            PreparedGeometryState::Desired(desired) => {
                let key = desired.key;
                self.desired_target = Some(desired);
                (key, ExactGeometryProgress::PendingIndex)
            }
            PreparedGeometryState::Target(active) => {
                let key = active.key;
                self.active = Some(active);
                (key, ExactGeometryProgress::Scanning)
            }
            PreparedGeometryState::Complete(target) => {
                let key = target.key;
                self.target = Some(target);
                (key, ExactGeometryProgress::TargetComplete)
            }
        };
        self.highest_job = Some(highest_job);
        if let Some(request) = highest_request {
            self.highest_request = Some(request);
        }
        if reset_object_request {
            self.highest_object_request = None;
        }
        self.high_water_bytes = self.high_water_bytes.max(admission_required_bytes);
        self.high_water_items = self.high_water_items.max(admission_required_items);
        ExactGeometryStart {
            key: job_key,
            progress,
            release,
            admission_required_bytes,
            admission_required_items,
        }
    }
}
