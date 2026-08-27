use super::*;

impl ExactGeometryOwner {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_response_text_page(
        &self,
        mut candidate: Box<ActiveJob>,
        page_end: ByteOffset,
        release: ExactGeometryRelease,
        text_system: &WindowTextSystem,
        shared: SharedOutput,
        mut budget: AdmissionBudget,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        let inputs = self.inputs.as_deref().expect("active owner retains inputs");
        let source_end = inputs.binding.extent().byte_len();
        let reached_source_end = page_end.get() == source_end;
        let target_ready = match &candidate.kind {
            ActiveKind::Target { target, anchor, .. } => {
                super::super::checkpoint::target_scan_ready(&candidate.scanner, *target, *anchor)
            }
            ActiveKind::Index => false,
        };
        if reached_source_end {
            super::super::scan::finalize_source(
                &mut candidate,
                text_system,
                &inputs.layout,
                &inputs.style,
                self.limits,
                source_end,
                &mut budget,
            )
            .map_err(|error| {
                prepared_failure(
                    error,
                    budget
                        .failure_stage
                        .unwrap_or(ExactGeometryFailureStage::Finalize),
                    &budget,
                )
            })?;
            if matches!(candidate.kind, ActiveKind::Index) {
                let terminal = super::super::checkpoint::make_checkpoint(
                    &candidate.scanner,
                    &inputs.layout,
                    true,
                )
                .map_err(|error| {
                    prepared_failure(error, ExactGeometryFailureStage::Checkpoint, &budget)
                })?;
                observe_prepared(
                    &mut budget,
                    &candidate,
                    size_of::<ExactGeometryCheckpoint>(),
                    1,
                )?;
                super::super::checkpoint::retain_checkpoint(
                    &mut candidate.scanner.checkpoints,
                    terminal,
                    self.limits.max_checkpoints,
                );
            }
        }
        candidate.page_use = ActivePageUse::Traverse { anchor: page_end };
        if !reached_source_end && !target_ready {
            observe_prepared(&mut budget, &candidate, 0, 0)?;
            return self.finish_active_target_response(
                candidate,
                ExactGeometryProgress::Scanning,
                release,
                shared,
                budget,
                successor,
            );
        }
        match candidate.kind.clone() {
            ActiveKind::Target {
                predecessor,
                predecessor_checkpoint,
                ..
            } => self.finish_target_publication(
                candidate,
                predecessor,
                predecessor_checkpoint,
                release,
                shared,
                budget,
            ),
            ActiveKind::Index => {
                self.finish_index_publication(candidate, release, shared, budget, successor)
            }
        }
    }

    pub(super) fn finish_target_publication(
        &self,
        mut delta: Box<ActiveJob>,
        predecessor: crate::SourcePosition,
        predecessor_checkpoint: ExactGeometryCheckpoint,
        mut release: ExactGeometryRelease,
        shared: SharedOutput,
        mut budget: AdmissionBudget,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        let current = self.response_active(delta.key, false)?;
        let record_count = current
            .scanner
            .fragments
            .len()
            .checked_add(delta.scanner.fragments.len())
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        let conversion_bytes = size_of::<BlockTargetPublication>()
            .checked_add(
                record_count
                    .checked_mul(size_of::<StreamingLayoutFragment>())
                    .ok_or_else(|| prepared_capacity_failure(&budget))?,
            )
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        let conversion_items = record_count
            .checked_add(1)
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        observe_prepared(&mut budget, &delta, conversion_bytes, conversion_items)?;
        let output_charge = accounting::add_fragment_charge(
            current.scanner.output_charge,
            delta.scanner.output_charge,
        )
        .map_err(|error| {
            prepared_failure(error, ExactGeometryFailureStage::Publication, &budget)
        })?;
        let output_item_charge = accounting::add_fragment_item_charge(
            current.scanner.output_item_charge,
            delta.scanner.output_item_charge,
        )
        .map_err(|error| {
            prepared_failure(error, ExactGeometryFailureStage::Publication, &budget)
        })?;
        let target_source = delta
            .scanner
            .target_source
            .unwrap_or(delta.scanner.target_line_position);
        let source_end = delta
            .scanner
            .continuation
            .next_position
            .try_into()
            .map_err(|_| {
                prepared_failure(
                    ExactGeometryError::SourceContract,
                    ExactGeometryFailureStage::Publication,
                    &budget,
                )
            })?;
        let fragments = Arc::from(
            current
                .scanner
                .fragments
                .iter()
                .cloned()
                .chain(delta.scanner.fragments.drain(..))
                .collect::<Vec<_>>(),
        );
        let target = Box::new(BlockTargetPublication {
            key: delta.key,
            predecessor,
            target_source,
            source_end,
            predecessor_checkpoint,
            visual_lines_lower_bound: delta.scanner.continuation.visual_lines,
            content_height_lower_bound: delta.scanner.continuation.block_offset
                + delta.scanner.continuation.line_block_extent,
            fragments,
            charge: output_charge,
            item_charge: output_item_charge,
        });
        let final_counts = accounting::counts(
            self.inputs.as_deref(),
            None,
            self.desired_target.as_deref(),
            self.index.as_deref(),
            Some(&target),
        );
        if checked_total_bytes(final_counts).is_err_and(|_| true)
            || checked_total_items(final_counts).is_err_and(|_| true)
            || checked_total_bytes(final_counts).unwrap_or(usize::MAX)
                > self.limits.max_retained_bytes
            || checked_total_items(final_counts).unwrap_or(usize::MAX)
                > self.limits.max_retained_items
        {
            return Err(prepared_failure(
                ExactGeometryError::CapacityExceeded,
                ExactGeometryFailureStage::Publication,
                &budget,
            ));
        }
        release.jobs.push(delta.key);
        release.counts = checked_add_counts(release.counts, completion_release_counts(&delta))
            .map_err(|_| prepared_capacity_failure(&budget))?;
        self.finish_target_response(
            PreparedTargetResponseState::CompleteTarget(target),
            None,
            ExactGeometryProgress::TargetComplete,
            release,
            shared,
            budget,
        )
    }

    pub(super) fn finish_index_publication(
        &self,
        delta: Box<ActiveJob>,
        mut release: ExactGeometryRelease,
        shared: SharedOutput,
        mut budget: AdmissionBudget,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        let current = self.response_active(delta.key, true)?;
        let initial_object_leading = delta.scanner.first_object_cursor.map(|cursor| {
            SourcePosition::new(
                cursor.anchor(),
                crate::InlineObjectGap::before(cursor.neighbor()),
            )
        });
        let mut checkpoints = VecDeque::with_capacity(self.limits.max_checkpoints);
        for checkpoint in current
            .scanner
            .checkpoints
            .iter()
            .chain(delta.scanner.checkpoints.iter())
        {
            let mut checkpoint = checkpoint.clone();
            if let Some(leading) = initial_object_leading
                && checkpoint.source.byte_offset == leading.byte_offset
                && matches!(checkpoint.source.gap, crate::InlineObjectGap::NoObjects)
            {
                checkpoint.source = leading;
                checkpoint.continuation.next_position = leading.into();
            }
            super::super::checkpoint::retain_checkpoint(
                &mut checkpoints,
                checkpoint,
                self.limits.max_checkpoints,
            );
        }
        let mut checkpoint_records = Vec::with_capacity(checkpoints.len());
        checkpoint_records.extend(checkpoints.iter().cloned());
        let destination_bytes = checkpoints
            .capacity()
            .checked_mul(size_of::<ExactGeometryCheckpoint>())
            .and_then(|bytes| {
                checkpoint_records
                    .capacity()
                    .checked_mul(size_of::<ExactGeometryCheckpoint>())
                    .and_then(|records| bytes.checked_add(records))
            })
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        let destination_items = checkpoints
            .capacity()
            .checked_add(checkpoint_records.capacity())
            .and_then(|items| items.checked_add(1))
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        observe_prepared(&mut budget, &delta, destination_bytes, destination_items)?;
        drop(checkpoints);
        let extent = self
            .inputs
            .as_deref()
            .expect("active owner retains inputs")
            .binding
            .extent()
            .byte_len();
        let aggregate = ExactGeometryAggregate {
            visual_lines: delta.scanner.continuation.visual_lines,
            content_height: delta.scanner.continuation.block_offset,
        };
        let document_selection = exact_document_selection(&delta.scanner, extent);
        let index = Box::new(ExactGeometryIndex {
            key: delta.key,
            aggregate,
            checkpoints: Arc::from(checkpoint_records),
            document_selection,
        });
        let mut target = successor.target;
        let mut anchor = successor.anchor;
        if successor.select_all {
            target = BlockTarget::new(
                aggregate.content_height,
                target.viewport_extent(),
                target.overscan(),
            );
            anchor = Some(document_selection.head);
        }
        let prepared_target = self
            .prepare_target_replacement_from_index(
                &index,
                successor.target_job_id,
                successor.page_id,
                target,
                anchor,
            )
            .map_err(|error| {
                prepared_failure(error, ExactGeometryFailureStage::Publication, &budget)
            })?;
        let index_counts = accounting::counts(None, None, None, Some(&index), None);
        let additional_bytes = index_counts
            .total_bytes()
            .checked_add(prepared_target.retained_bytes())
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        let additional_items = index_counts
            .total_items()
            .checked_add(prepared_target.retained_items())
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        observe_prepared(&mut budget, &delta, additional_bytes, additional_items)?;
        release.jobs.push(delta.key);
        release.counts = checked_add_counts(release.counts, completion_release_counts(&delta))
            .map_err(|_| prepared_capacity_failure(&budget))?;
        if let Some(prior) = self.index.as_deref() {
            release.jobs.push(prior.key);
            let mut counts = ExactGeometryCounts::default();
            counts.publication_bytes = size_of::<ExactGeometryIndex>();
            counts.publication_items = 1;
            counts.checkpoints = prior.checkpoints.len();
            counts.checkpoint_bytes = accounting::checkpoint_record_bytes(prior.checkpoints.len());
            release.counts = checked_add_counts(release.counts, counts)
                .map_err(|_| prepared_capacity_failure(&budget))?;
        }
        for key in &prepared_target.release().jobs {
            if !release.jobs.contains(key) {
                release.jobs.push(*key);
            }
        }
        for key in &prepared_target.release().pages {
            if !release.pages.contains(key) {
                release.pages.push(*key);
            }
        }
        for key in &prepared_target.release().object_pages {
            if !release.object_pages.contains(key) {
                release.object_pages.push(*key);
            }
        }
        release.counts = checked_add_counts(release.counts, prepared_target.release().counts)
            .map_err(|_| prepared_capacity_failure(&budget))?;
        release.jobs.sort();
        release.jobs.dedup();
        let progress = if prepared_target.terminal_target().is_some() {
            ExactGeometryProgress::TargetComplete
        } else {
            ExactGeometryProgress::Scanning
        };
        let prepared_successor = prepared_target
            .page_request()
            .map(PreparedTargetSuccessor::Page);
        self.finish_target_response(
            PreparedTargetResponseState::CompleteIndex {
                index,
                target: prepared_target,
            },
            prepared_successor,
            progress,
            release,
            shared,
            budget,
        )
    }

    pub(super) fn finish_active_target_response(
        &self,
        mut delta: Box<ActiveJob>,
        progress: ExactGeometryProgress,
        release: ExactGeometryRelease,
        shared: SharedOutput,
        mut budget: AdmissionBudget,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        let successor = self
            .prepare_target_successor(&mut delta, progress, successor)
            .map_err(|error| prepared_failure(error, ExactGeometryFailureStage::Scan, &budget))?;
        let is_index = matches!(delta.kind, ActiveKind::Index);
        let current = self.response_active(delta.key, is_index)?;
        let fragment_capacity = current
            .scanner
            .fragments
            .len()
            .checked_add(delta.scanner.fragments.len())
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        let fragments = Vec::with_capacity(fragment_capacity);
        let checkpoint_capacity = current
            .scanner
            .checkpoints
            .len()
            .checked_add(delta.scanner.checkpoints.len())
            .map(|capacity| capacity.min(self.limits.max_checkpoints))
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        let checkpoints = VecDeque::with_capacity(checkpoint_capacity);
        let destination_bytes = fragments
            .capacity()
            .checked_mul(size_of::<StreamingLayoutFragment>())
            .and_then(|bytes| {
                checkpoints
                    .capacity()
                    .checked_mul(size_of::<ExactGeometryCheckpoint>())
                    .and_then(|checkpoints| bytes.checked_add(checkpoints))
            })
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        let destination_items = fragments
            .capacity()
            .checked_add(checkpoints.capacity())
            .ok_or_else(|| prepared_capacity_failure(&budget))?;
        observe_prepared(&mut budget, &delta, destination_bytes, destination_items)?;
        let output_charge = accounting::add_fragment_charge(
            current.scanner.output_charge,
            delta.scanner.output_charge,
        )
        .map_err(|error| prepared_failure(error, ExactGeometryFailureStage::Scan, &budget))?;
        let output_item_charge = accounting::add_fragment_item_charge(
            current.scanner.output_item_charge,
            delta.scanner.output_item_charge,
        )
        .map_err(|error| prepared_failure(error, ExactGeometryFailureStage::Scan, &budget))?;
        self.finish_target_response(
            PreparedTargetResponseState::Active(PreparedActiveTarget {
                delta,
                fragments,
                checkpoints,
                output_charge,
                output_item_charge,
            }),
            Some(successor),
            progress,
            release,
            shared,
            budget,
        )
    }

    pub(super) fn prepare_target_successor(
        &self,
        active: &mut ActiveJob,
        progress: ExactGeometryProgress,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetSuccessor, ExactGeometryError> {
        if active.pending.is_some() {
            return Err(ExactGeometryError::PageAlreadyPending);
        }
        let inputs = self.inputs()?;
        match progress {
            ExactGeometryProgress::Scanning => {
                if active.text_page.is_some()
                    || self
                        .highest_request
                        .is_some_and(|highest| successor.page_id <= highest)
                {
                    return Err(if active.text_page.is_some() {
                        ExactGeometryError::PageAlreadyPending
                    } else {
                        ExactGeometryError::IdNotMonotonic
                    });
                }
                let (anchor, direction) = match active.page_use {
                    ActivePageUse::Traverse { anchor } => (anchor, PageDirection::Forward),
                    ActivePageUse::Context { required_end, .. } => {
                        (required_end, PageDirection::Backward)
                    }
                };
                let key = PageRequestKey::adjacent(
                    successor.page_id,
                    inputs.binding.binding(),
                    inputs.binding.revision(),
                    match &active.kind {
                        ActiveKind::Index => crate::PagePurpose::GeometryIndex,
                        ActiveKind::Target { .. } => crate::PagePurpose::GeometryTarget,
                    },
                    anchor,
                    direction,
                    self.limits.max_page_bytes,
                )
                .map_err(|_| ExactGeometryError::InvalidLimits)?;
                active.pending = Some(Box::new(PendingInput::Text(key)));
                Ok(PreparedTargetSuccessor::Page(PageRequest::new(key)))
            }
            ExactGeometryProgress::NeedObjects => {
                if self
                    .highest_object_request
                    .is_some_and(|highest| successor.object_id <= highest)
                {
                    return Err(ExactGeometryError::IdNotMonotonic);
                }
                let page = active.text_page.ok_or(ExactGeometryError::WrongInputKind)?;
                let cursor = active
                    .scanner
                    .object_cursor
                    .filter(|cursor| page.range.contains_offset(cursor.anchor()));
                let demand = ObjectDemandEnvelope::range(
                    page.range,
                    cursor,
                    ObjectDirection::Forward,
                    successor.max_objects,
                    successor.max_object_bytes,
                )
                .map_err(|_| ExactGeometryError::SourceContract)?;
                let key = ObjectRequestKey::new(
                    successor.object_id,
                    inputs.binding.binding(),
                    inputs.binding.revision(),
                    inputs.presentation_generation,
                    match &active.kind {
                        ActiveKind::Index => crate::ObjectPurpose::GeometryIndex,
                        ActiveKind::Target { .. } => crate::ObjectPurpose::GeometryTarget,
                    },
                    demand,
                )
                .map_err(|_| ExactGeometryError::SourceContract)?;
                active.pending = Some(Box::new(PendingInput::Object(key)));
                Ok(PreparedTargetSuccessor::Object {
                    request: ObjectRequest::new(key),
                    text_page: page.id,
                })
            }
            _ => Err(ExactGeometryError::WrongInputKind),
        }
    }

    pub(super) fn prepared_budget(
        &self,
        _candidate: &ActiveJob,
        _shared: SharedOutput,
        page_payload_bytes: usize,
        page_items: usize,
    ) -> Result<AdmissionBudget, ExactGeometryFailure> {
        let counts = self.counts();
        Ok(AdmissionBudget {
            fixed_bytes: checked_total_bytes(counts).map_err(|_| {
                self.prepared_validation_failure(ExactGeometryError::CapacityExceeded)
            })?,
            fixed_items: checked_total_items(counts).map_err(|_| {
                self.prepared_validation_failure(ExactGeometryError::CapacityExceeded)
            })?,
            page_payload_bytes,
            page_items,
            max_bytes: self.limits.max_retained_bytes,
            max_items: self.limits.max_retained_items,
            peak_bytes: 0,
            peak_items: 0,
            failure_stage: None,
        })
    }

    pub(super) fn finish_target_response(
        &self,
        state: PreparedTargetResponseState,
        successor: Option<PreparedTargetSuccessor>,
        progress: ExactGeometryProgress,
        release: ExactGeometryRelease,
        shared: SharedOutput,
        budget: AdmissionBudget,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        let (state_bytes, state_items) = match &state {
            PreparedTargetResponseState::Active(active) => {
                let counts = accounting::active_counts(&active.delta);
                let fragment_bytes = active
                    .fragments
                    .capacity()
                    .checked_mul(size_of::<StreamingLayoutFragment>())
                    .ok_or_else(|| prepared_capacity_failure(&budget))?;
                let destination_bytes = active
                    .checkpoints
                    .capacity()
                    .checked_mul(size_of::<ExactGeometryCheckpoint>())
                    .and_then(|bytes| bytes.checked_add(fragment_bytes))
                    .ok_or_else(|| prepared_capacity_failure(&budget))?;
                (
                    checked_total_bytes(counts)
                        .and_then(|bytes| bytes.checked_add(destination_bytes).ok_or(())),
                    checked_total_items(counts)
                        .and_then(|items| items.checked_add(active.fragments.capacity()).ok_or(()))
                        .and_then(|items| {
                            items.checked_add(active.checkpoints.capacity()).ok_or(())
                        }),
                )
            }
            PreparedTargetResponseState::CompleteTarget(target) => {
                let counts = accounting::target_counts(target);
                (
                    checked_total_bytes(counts)
                        .and_then(|bytes| bytes.checked_sub(shared.payload_bytes).ok_or(())),
                    checked_total_items(counts)
                        .and_then(|items| items.checked_sub(shared.semantic_items).ok_or(()))
                        .and_then(|items| items.checked_add(shared.fragment_records).ok_or(())),
                )
            }
            PreparedTargetResponseState::CompleteIndex { index, target } => {
                let counts = accounting::counts(None, None, None, Some(index), None);
                (
                    checked_total_bytes(counts)
                        .and_then(|bytes| bytes.checked_add(target.retained_bytes()).ok_or(())),
                    checked_total_items(counts)
                        .and_then(|items| items.checked_add(target.retained_items()).ok_or(())),
                )
            }
        };
        let release_bytes =
            release_storage_bytes(&release).map_err(|_| prepared_capacity_failure(&budget))?;
        let release_items =
            release_storage_items(&release).map_err(|_| prepared_capacity_failure(&budget))?;
        let retained_bytes = state_bytes
            .and_then(|bytes| bytes.checked_add(release_bytes).ok_or(()))
            .map_err(|_| prepared_capacity_failure(&budget))?;
        let retained_items = state_items
            .and_then(|items| items.checked_add(release_items).ok_or(()))
            .map_err(|_| prepared_capacity_failure(&budget))?;
        Ok(PreparedTargetResponse {
            state,
            successor,
            progress,
            release,
            retained_bytes,
            retained_items,
            admission_required_bytes: budget.peak_bytes,
            admission_required_items: budget.peak_items,
        })
    }

    pub(super) fn prepared_validation_failure(
        &self,
        error: ExactGeometryError,
    ) -> ExactGeometryFailure {
        ExactGeometryFailure {
            error,
            stage: ExactGeometryFailureStage::Validation,
            release: ExactGeometryRelease::default(),
            admission_required_bytes: self.counts().total_bytes(),
            admission_required_items: self.counts().total_items(),
        }
    }

    pub(crate) fn commit_prepared_target_response(
        &mut self,
        prepared: PreparedTargetResponse,
    ) -> ExactGeometryAdmission {
        let PreparedTargetResponse {
            state,
            successor,
            progress,
            release,
            admission_required_bytes,
            admission_required_items,
            ..
        } = prepared;
        match state {
            PreparedTargetResponseState::Active(mut prepared) => {
                let current = self
                    .active
                    .take()
                    .expect("prepared target response retains a current active job");
                debug_assert_eq!(current.key, prepared.delta.key);
                let mut current = *current;
                let required = current
                    .scanner
                    .fragments
                    .len()
                    .checked_add(prepared.delta.scanner.fragments.len())
                    .expect("prepared fragment count was checked");
                debug_assert!(prepared.fragments.capacity() >= required);
                prepared
                    .fragments
                    .extend(current.scanner.fragments.drain(..));
                prepared
                    .fragments
                    .extend(prepared.delta.scanner.fragments.drain(..));
                for checkpoint in current
                    .scanner
                    .checkpoints
                    .drain(..)
                    .chain(prepared.delta.scanner.checkpoints.drain(..))
                {
                    super::super::checkpoint::retain_checkpoint(
                        &mut prepared.checkpoints,
                        checkpoint,
                        self.limits.max_checkpoints,
                    );
                }
                prepared.delta.scanner.fragments = prepared.fragments;
                prepared.delta.scanner.checkpoints = prepared.checkpoints;
                prepared.delta.scanner.output_charge = prepared.output_charge;
                prepared.delta.scanner.output_item_charge = prepared.output_item_charge;
                self.active = Some(prepared.delta);
            }
            PreparedTargetResponseState::CompleteTarget(target) => {
                let current = self
                    .active
                    .take()
                    .expect("prepared terminal target retains a current active job");
                debug_assert_eq!(current.key, target.key);
                debug_assert!(self.target.is_none());
                self.target = Some(target);
            }
            PreparedTargetResponseState::CompleteIndex { index, target } => {
                let current = self
                    .active
                    .take()
                    .expect("prepared index response retains a current active job");
                debug_assert_eq!(current.key, index.key);
                self.index = Some(index);
                let start = self.commit_prepared_transition(target);
                debug_assert_eq!(start.progress(), progress);
            }
        }
        match successor {
            Some(PreparedTargetSuccessor::Page(request)) => {
                self.highest_request = Some(request.key().id());
            }
            Some(PreparedTargetSuccessor::Object { request, .. }) => {
                self.highest_object_request = Some(request.key().id());
            }
            None => {}
        }
        self.high_water_bytes = self.high_water_bytes.max(admission_required_bytes);
        self.high_water_items = self.high_water_items.max(admission_required_items);
        self.observe_current();
        ExactGeometryAdmission {
            progress,
            release,
            admission_required_bytes,
            admission_required_items,
        }
    }
}
