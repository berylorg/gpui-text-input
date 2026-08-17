use std::{collections::VecDeque, mem::size_of, sync::Arc};

use gpui::{StreamingLayoutFragment, WindowTextSystem};

use crate::{
    ByteOffset, GeometryJobId, InlineObjectGap, ObjectDemandEnvelope, ObjectDirection, ObjectPage,
    ObjectRequest, ObjectRequestId, ObjectRequestKey, PageDemandEnvelope, PageDirection,
    PageEdgeFact, PageRequest, PageRequestId, PageRequestKey, RangePage, RangeSourceSelection,
    SourcePosition,
};

use super::{
    ActiveJob, ActiveKind, ActivePageUse, ActiveTextPage, AdmissionBudget, BlockTarget,
    BlockTargetPublication, DeferredObject, ExactGeometryAdmission, ExactGeometryAggregate,
    ExactGeometryCheckpoint, ExactGeometryCounts, ExactGeometryError, ExactGeometryFailure,
    ExactGeometryFailureStage, ExactGeometryIndex, ExactGeometryOwner, ExactGeometryProgress,
    ExactGeometryRelease, PendingInput, PreparedGeometryTransition, Scanner, accounting,
};

mod publication;

#[derive(Debug)]
pub(crate) struct PreparedTargetResponse {
    state: PreparedTargetResponseState,
    successor: Option<PreparedTargetSuccessor>,
    progress: ExactGeometryProgress,
    release: ExactGeometryRelease,
    retained_bytes: usize,
    retained_items: usize,
    admission_required_bytes: usize,
    admission_required_items: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetResponseSuccessor {
    pub(crate) target_job_id: GeometryJobId,
    pub(crate) page_id: PageRequestId,
    pub(crate) object_id: ObjectRequestId,
    pub(crate) max_objects: usize,
    pub(crate) max_object_bytes: usize,
    pub(crate) target: BlockTarget,
    pub(crate) anchor: Option<SourcePosition>,
    pub(crate) select_all: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PreparedTargetSuccessor {
    Page(PageRequest),
    Object {
        request: ObjectRequest,
        text_page: crate::PageId,
    },
}

#[derive(Debug)]
enum PreparedTargetResponseState {
    Active(PreparedActiveTarget),
    CompleteTarget(Box<BlockTargetPublication>),
    CompleteIndex {
        index: Box<ExactGeometryIndex>,
        target: PreparedGeometryTransition,
    },
}

#[derive(Debug)]
struct PreparedActiveTarget {
    delta: Box<ActiveJob>,
    fragments: Vec<StreamingLayoutFragment>,
    checkpoints: VecDeque<ExactGeometryCheckpoint>,
    output_charge: gpui::StreamingLayoutCharge,
    output_item_charge: gpui::StreamingLayoutItemCharge,
}

impl PreparedTargetResponse {
    pub(crate) const fn key(&self) -> crate::GeometryJobKey {
        match &self.state {
            PreparedTargetResponseState::Active(active) => active.delta.key,
            PreparedTargetResponseState::CompleteTarget(target) => target.key,
            PreparedTargetResponseState::CompleteIndex { target, .. } => target.key(),
        }
    }

    pub(crate) const fn progress(&self) -> ExactGeometryProgress {
        self.progress
    }

    pub(crate) const fn release(&self) -> &ExactGeometryRelease {
        &self.release
    }

    pub(crate) fn terminal_target(&self) -> Option<&BlockTargetPublication> {
        match &self.state {
            PreparedTargetResponseState::CompleteTarget(target) => Some(target),
            PreparedTargetResponseState::CompleteIndex { target, .. } => target.terminal_target(),
            PreparedTargetResponseState::Active(_) => None,
        }
    }

    pub(crate) fn terminal_index(&self) -> Option<&ExactGeometryIndex> {
        match &self.state {
            PreparedTargetResponseState::CompleteIndex { index, .. } => Some(index),
            _ => None,
        }
    }

    pub(crate) const fn successor(&self) -> Option<PreparedTargetSuccessor> {
        self.successor
    }

    pub(crate) const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub(crate) const fn retained_items(&self) -> usize {
        self.retained_items
    }
}

#[derive(Clone, Copy)]
struct SharedOutput {
    payload_bytes: usize,
    semantic_items: usize,
    fragment_records: usize,
}

fn copy_response_continuation(
    active: &ActiveJob,
) -> Result<(Box<ActiveJob>, SharedOutput), ExactGeometryError> {
    let is_target = matches!(active.kind, ActiveKind::Target { .. });
    if is_target && !active.scanner.checkpoints.is_empty()
        || !is_target && !active.scanner.fragments.is_empty()
    {
        return Err(ExactGeometryError::SourceContract);
    }
    let scanner = Scanner {
        cursor: active.scanner.cursor.clone(),
        cursor_origin: active.scanner.cursor_origin,
        grapheme_start_cursor: active.scanner.grapheme_start_cursor.clone(),
        continuation: active.scanner.continuation,
        continuation_items: active.scanner.continuation_items,
        logical_line: active.scanner.logical_line,
        segment_text: active.scanner.segment_text.clone(),
        segment_start: active.scanner.segment_start,
        grapheme_text: active.scanner.grapheme_text.clone(),
        grapheme_start: active.scanner.grapheme_start,
        read_position: active.scanner.read_position,
        active_atom: active.scanner.active_atom.as_deref().copied().map(Box::new),
        checkpoints: Default::default(),
        fragments: Vec::new(),
        output_charge: Default::default(),
        output_item_charge: Default::default(),
        target_line_position: active.scanner.target_line_position,
        target_line_block: active.scanner.target_line_block,
        target_source: active.scanner.target_source,
        first_object_cursor: active.scanner.first_object_cursor,
        object_cursor: active.scanner.object_cursor,
        deferred_object: active.scanner.deferred_object.as_deref().map(|object| {
            Box::new(DeferredObject {
                binding: object.binding,
                presentation_generation: object.presentation_generation,
                fact: object.fact.clone(),
            })
        }),
    };
    let shared = if is_target {
        SharedOutput {
            payload_bytes: active.scanner.output_charge.total()?,
            semantic_items: active.scanner.output_item_charge.total()?,
            fragment_records: active.scanner.fragments.len(),
        }
    } else {
        SharedOutput {
            payload_bytes: 0,
            semantic_items: 0,
            fragment_records: 0,
        }
    };
    Ok((
        Box::new(ActiveJob {
            key: active.key,
            kind: active.kind.clone(),
            page_use: active.page_use,
            pending: active.pending.as_deref().copied().map(Box::new),
            text_page: active.text_page,
            window_identity: active.window_identity,
            retained_capacity: active.retained_capacity,
            scanner,
        }),
        shared,
    ))
}

fn exact_document_selection(scanner: &Scanner, extent: u64) -> RangeSourceSelection {
    let anchor = scanner
        .first_object_cursor
        .filter(|cursor| cursor.anchor().get() == 0)
        .map_or_else(
            || SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects),
            |cursor| {
                SourcePosition::new(
                    ByteOffset::new(0),
                    InlineObjectGap::before(cursor.neighbor()),
                )
            },
        );
    let head = scanner
        .object_cursor
        .filter(|cursor| cursor.anchor().get() == extent)
        .map_or_else(
            || SourcePosition::new(ByteOffset::new(extent), InlineObjectGap::NoObjects),
            |cursor| {
                SourcePosition::new(
                    ByteOffset::new(extent),
                    InlineObjectGap::after(cursor.neighbor()),
                )
            },
        );
    RangeSourceSelection { anchor, head }
}

impl ExactGeometryOwner {
    pub(crate) fn prepare_target_page(
        &self,
        key: crate::GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        self.prepare_response_page_inner(key, page, text_system, false, false, successor)
    }

    pub(crate) fn prepare_target_resident_page(
        &self,
        key: crate::GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        self.prepare_response_page_inner(key, page, text_system, true, false, successor)
    }

    pub(crate) fn prepare_index_page(
        &self,
        key: crate::GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        self.prepare_response_page_inner(key, page, text_system, false, true, successor)
    }

    pub(crate) fn prepare_index_resident_page(
        &self,
        key: crate::GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        self.prepare_response_page_inner(key, page, text_system, true, true, successor)
    }

    fn prepare_response_page_inner(
        &self,
        key: crate::GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
        resident: bool,
        index: bool,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        let active = self.response_active(key, index)?;
        let Some(PendingInput::Text(expected)) = active.pending.as_deref().copied() else {
            return Err(self.prepared_validation_failure(ExactGeometryError::NoActiveJob));
        };
        if (!resident && page.key() != expected)
            || (resident && !resident_page_satisfies(page, expected))
        {
            return Err(self.prepared_validation_failure(ExactGeometryError::WrongPage(page.key())));
        }
        let (mut candidate, shared) = copy_response_continuation(active)
            .map_err(|error| self.prepared_validation_failure(error))?;
        let mut budget = self.prepared_budget(
            &candidate,
            shared,
            page.retained_charge().bytes(),
            page.retained_charge().items(),
        )?;
        observe_prepared(&mut budget, &candidate, 0, 0)?;
        if page.range().len() > self.limits.max_page_bytes {
            return Err(prepared_failure(
                ExactGeometryError::CapacityExceeded,
                ExactGeometryFailureStage::PageCoexistence,
                &budget,
            ));
        }
        let window_identity = text_system as *const WindowTextSystem as usize;
        if candidate
            .window_identity
            .is_some_and(|identity| identity != window_identity)
        {
            return Err(prepared_failure(
                ExactGeometryError::SourceContract,
                ExactGeometryFailureStage::WindowIdentity,
                &budget,
            ));
        }
        candidate.window_identity = Some(window_identity);
        let source_end = self
            .inputs
            .as_deref()
            .expect("active owner retains inputs")
            .binding
            .extent()
            .byte_len();
        let requested_edge_matches = match candidate.page_use {
            ActivePageUse::Traverse { anchor } => page.range().start() == anchor,
            ActivePageUse::Context { required_end, .. } => page.range().end() == required_end,
        };
        let malformed_edges = (page.preceding() == PageEdgeFact::DocumentBoundary)
            != (page.range().start().get() == 0)
            || (page.following() == PageEdgeFact::DocumentBoundary)
                != (page.range().end().get() == source_end)
            || page.end_of_source() != (page.range().end().get() == source_end)
            || !requested_edge_matches
            || page.range().end().get() > source_end;
        if malformed_edges {
            return Err(prepared_failure(
                ExactGeometryError::SourceContract,
                ExactGeometryFailureStage::PageCoexistence,
                &budget,
            ));
        }
        if let ActivePageUse::Context { replay, .. } = candidate.page_use {
            prepare_context_page(&mut candidate, page, replay).map_err(|error| {
                prepared_failure(error, ExactGeometryFailureStage::Scan, &budget)
            })?;
            observe_prepared(&mut budget, &candidate, 0, 0)?;
            return self.finish_active_target_response(
                candidate,
                ExactGeometryProgress::Scanning,
                consumed_page_release(expected),
                shared,
                budget,
                successor,
            );
        }
        candidate.pending = None;
        candidate.text_page = Some(ActiveTextPage {
            id: page.id(),
            range: page.range(),
        });
        observe_prepared(&mut budget, &candidate, 0, 0)?;
        self.finish_active_target_response(
            candidate,
            ExactGeometryProgress::NeedObjects,
            consumed_page_release(expected),
            shared,
            budget,
            successor,
        )
    }

    fn response_active(
        &self,
        key: crate::GeometryJobKey,
        index: bool,
    ) -> Result<&ActiveJob, ExactGeometryFailure> {
        let Some(active) = self.active.as_deref() else {
            return Err(self.prepared_validation_failure(ExactGeometryError::ObsoleteJob(key)));
        };
        if active.key != key {
            return Err(self.prepared_validation_failure(ExactGeometryError::ObsoleteJob(key)));
        }
        if matches!(active.kind, ActiveKind::Index) != index {
            return Err(self.prepared_validation_failure(ExactGeometryError::WrongInputKind));
        }
        Ok(active)
    }

    pub(crate) fn prepare_target_object_page(
        &self,
        key: crate::GeometryJobKey,
        text_page: &RangePage,
        object_page: &ObjectPage,
        text_system: &WindowTextSystem,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        self.prepare_response_object_page(
            key,
            text_page,
            object_page,
            text_system,
            false,
            false,
            successor,
        )
    }

    pub(crate) fn prepare_target_resident_object_page(
        &self,
        key: crate::GeometryJobKey,
        text_page: &RangePage,
        object_page: &ObjectPage,
        text_system: &WindowTextSystem,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        self.prepare_response_object_page(
            key,
            text_page,
            object_page,
            text_system,
            false,
            true,
            successor,
        )
    }

    pub(crate) fn prepare_index_object_page(
        &self,
        key: crate::GeometryJobKey,
        text_page: &RangePage,
        object_page: &ObjectPage,
        text_system: &WindowTextSystem,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        self.prepare_response_object_page(
            key,
            text_page,
            object_page,
            text_system,
            true,
            false,
            successor,
        )
    }

    pub(crate) fn prepare_index_resident_object_page(
        &self,
        key: crate::GeometryJobKey,
        text_page: &RangePage,
        object_page: &ObjectPage,
        text_system: &WindowTextSystem,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        self.prepare_response_object_page(
            key,
            text_page,
            object_page,
            text_system,
            true,
            true,
            successor,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_response_object_page(
        &self,
        key: crate::GeometryJobKey,
        text_page: &RangePage,
        object_page: &ObjectPage,
        text_system: &WindowTextSystem,
        index: bool,
        resident: bool,
        successor: TargetResponseSuccessor,
    ) -> Result<PreparedTargetResponse, ExactGeometryFailure> {
        let active = self.response_active(key, index)?;
        let Some(PendingInput::Object(expected)) = active.pending.as_deref().copied() else {
            return Err(self.prepared_validation_failure(ExactGeometryError::WrongInputKind));
        };
        let Some(active_page) = active.text_page else {
            return Err(self.prepared_validation_failure(ExactGeometryError::WrongInputKind));
        };
        if text_page.id() != active_page.id
            || text_page.range() != active_page.range
            || if resident {
                !resident_object_payload_satisfies(object_page, expected)
            } else {
                !resident_object_page_satisfies(object_page, expected)
            }
        {
            return Err(
                self.prepared_validation_failure(ExactGeometryError::WrongObjectPage(
                    object_page.key(),
                )),
            );
        }
        let page_bytes = text_page
            .retained_charge()
            .bytes()
            .checked_add(object_page.retained_charge().bytes())
            .ok_or_else(|| {
                self.prepared_validation_failure(ExactGeometryError::CapacityExceeded)
            })?;
        let page_items = text_page
            .retained_charge()
            .items()
            .checked_add(object_page.objects().len())
            .and_then(|items| items.checked_add(1))
            .ok_or_else(|| {
                self.prepared_validation_failure(ExactGeometryError::CapacityExceeded)
            })?;
        let (mut candidate, shared) = copy_response_continuation(active)
            .map_err(|error| self.prepared_validation_failure(error))?;
        let mut budget = self.prepared_budget(&candidate, shared, page_bytes, page_items)?;
        observe_prepared(&mut budget, &candidate, 0, 0)?;
        let inputs = self.inputs.as_deref().expect("active owner retains inputs");
        let source_end = inputs.binding.extent().byte_len();
        let scan = super::scan::process_object_page(
            &mut candidate,
            text_page,
            object_page,
            text_system,
            inputs,
            self.limits,
            source_end,
            &mut budget,
        )
        .map_err(|error| {
            prepared_failure(
                error,
                budget
                    .failure_stage
                    .unwrap_or(ExactGeometryFailureStage::Scan),
                &budget,
            )
        })?;
        candidate.pending = None;
        let release = consumed_object_release(expected);
        if let super::scan::PageScan::NeedContext {
            required_end,
            replay,
        } = scan
        {
            candidate.page_use = ActivePageUse::Context {
                required_end,
                replay,
            };
            candidate.text_page = None;
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
        if !object_page.complete() {
            observe_prepared(&mut budget, &candidate, 0, 0)?;
            return self.finish_active_target_response(
                candidate,
                ExactGeometryProgress::NeedObjects,
                release,
                shared,
                budget,
                successor,
            );
        }
        candidate.text_page = None;
        self.finish_response_text_page(
            candidate,
            active_page.range.end(),
            release,
            text_system,
            shared,
            budget,
            successor,
        )
    }
}

fn observe_prepared(
    budget: &mut AdmissionBudget,
    active: &ActiveJob,
    transient_bytes: usize,
    transient_items: usize,
) -> Result<(), ExactGeometryFailure> {
    let counts = accounting::active_counts(active);
    let bytes = budget
        .fixed_bytes
        .checked_add(budget.page_payload_bytes)
        .and_then(|value| value.checked_add(checked_total_bytes(counts).ok()?))
        .and_then(|value| value.checked_add(transient_bytes))
        .ok_or_else(|| prepared_capacity_failure(budget))?;
    let items = budget
        .fixed_items
        .checked_add(budget.page_items)
        .and_then(|value| value.checked_add(checked_total_items(counts).ok()?))
        .and_then(|value| value.checked_add(transient_items))
        .ok_or_else(|| prepared_capacity_failure(budget))?;
    budget.peak_bytes = budget.peak_bytes.max(bytes);
    budget.peak_items = budget.peak_items.max(items);
    if bytes > budget.max_bytes || items > budget.max_items {
        Err(prepared_failure(
            ExactGeometryError::CapacityExceeded,
            ExactGeometryFailureStage::PageCoexistence,
            budget,
        ))
    } else {
        Ok(())
    }
}

fn prepared_failure(
    error: ExactGeometryError,
    stage: ExactGeometryFailureStage,
    budget: &AdmissionBudget,
) -> ExactGeometryFailure {
    ExactGeometryFailure {
        error,
        stage,
        release: ExactGeometryRelease::default(),
        admission_required_bytes: budget.peak_bytes,
        admission_required_items: budget.peak_items,
    }
}

fn prepared_capacity_failure(budget: &AdmissionBudget) -> ExactGeometryFailure {
    prepared_failure(
        ExactGeometryError::CapacityExceeded,
        ExactGeometryFailureStage::PageCoexistence,
        budget,
    )
}

fn prepare_context_page(
    active: &mut ActiveJob,
    page: &RangePage,
    replay: ByteOffset,
) -> Result<(), ExactGeometryError> {
    let origin = active.scanner.cursor_origin.get();
    let feed_start = page.range().start().get().max(origin);
    let malformed = feed_start >= page.range().end().get()
        || page.atoms().iter().any(|atom| {
            atom.fragment_range().end().get() > feed_start
                && atom.fragment_range().start().get() < page.range().end().get()
        });
    if malformed {
        return Err(ExactGeometryError::SourceContract);
    }
    let local_start = usize::try_from(feed_start - page.range().start().get())
        .map_err(|_| ExactGeometryError::SourceContract)?;
    let chunk_start =
        usize::try_from(feed_start - origin).map_err(|_| ExactGeometryError::SourceContract)?;
    active
        .scanner
        .cursor
        .provide_context(&page.text()[local_start..], chunk_start);
    active.page_use = ActivePageUse::Traverse { anchor: replay };
    active.pending = None;
    active.text_page = None;
    Ok(())
}

fn consumed_page_release(page: PageRequestKey) -> ExactGeometryRelease {
    ExactGeometryRelease {
        pages: vec![page],
        counts: ExactGeometryCounts {
            pending_page_items: 1,
            pending_page_bytes: size_of::<PageRequestKey>(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn consumed_object_release(page: ObjectRequestKey) -> ExactGeometryRelease {
    ExactGeometryRelease {
        object_pages: vec![page],
        counts: ExactGeometryCounts {
            pending_object_page_items: 1,
            pending_object_page_bytes: size_of::<ObjectRequestKey>(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn resident_object_page_satisfies(page: &ObjectPage, expected: ObjectRequestKey) -> bool {
    let actual = page.key();
    actual.id() == expected.id()
        && actual.binding() == expected.binding()
        && actual.revision() == expected.revision()
        && actual.presentation_generation() == expected.presentation_generation()
        && actual.purpose() == expected.purpose()
        && actual.demand() == expected.demand()
}

fn resident_object_payload_satisfies(page: &ObjectPage, expected: ObjectRequestKey) -> bool {
    let actual = page.key();
    actual.binding() == expected.binding()
        && actual.revision() == expected.revision()
        && actual.presentation_generation() == expected.presentation_generation()
        && actual.demand() == expected.demand()
}

fn resident_page_satisfies(page: &RangePage, expected: PageRequestKey) -> bool {
    if page.key().binding() != expected.binding()
        || page.key().revision() != expected.revision()
        || page.range().len() > expected.max_payload_bytes()
    {
        return false;
    }
    let PageDemandEnvelope::Adjacent {
        anchor, direction, ..
    } = expected.demand()
    else {
        return false;
    };
    let anchored = match direction {
        PageDirection::Forward => page.range().start() == anchor,
        PageDirection::Backward => page.range().end() == anchor,
    };
    let progresses_or_matches_edge = !page.range().is_empty()
        || match direction {
            PageDirection::Forward => page.following() == PageEdgeFact::DocumentBoundary,
            PageDirection::Backward => page.preceding() == PageEdgeFact::DocumentBoundary,
        };
    anchored && progresses_or_matches_edge
}

fn completion_release_counts(active: &ActiveJob) -> ExactGeometryCounts {
    let mut counts = accounting::active_counts(active);
    counts.pending_page_bytes = 0;
    counts.pending_page_items = 0;
    counts.pending_object_page_bytes = 0;
    counts.pending_object_page_items = 0;
    counts.output_items = 0;
    counts.output_record_bytes = 0;
    counts.output_payload_bytes = 0;
    counts
}

fn release_storage_bytes(release: &ExactGeometryRelease) -> Result<usize, ()> {
    release
        .jobs
        .capacity()
        .checked_mul(size_of::<crate::GeometryJobKey>())
        .and_then(|bytes| {
            release
                .pages
                .capacity()
                .checked_mul(size_of::<PageRequestKey>())
                .and_then(|right| bytes.checked_add(right))
        })
        .and_then(|bytes| {
            release
                .object_pages
                .capacity()
                .checked_mul(size_of::<ObjectRequestKey>())
                .and_then(|right| bytes.checked_add(right))
        })
        .ok_or(())
}

fn release_storage_items(release: &ExactGeometryRelease) -> Result<usize, ()> {
    release
        .jobs
        .capacity()
        .checked_add(release.pages.capacity())
        .and_then(|items| items.checked_add(release.object_pages.capacity()))
        .ok_or(())
}

fn checked_total_bytes(counts: ExactGeometryCounts) -> Result<usize, ()> {
    checked_sum([
        counts.owner_bytes,
        counts.input_bytes,
        counts.desired_target_bytes,
        counts.active_job_bytes,
        counts.pending_page_bytes,
        counts.pending_object_page_bytes,
        counts.scan_buffer_bytes,
        counts.active_atom_bytes,
        counts.deferred_object_bytes,
        counts.checkpoint_bytes,
        counts.continuation_bytes,
        counts.output_record_bytes,
        counts.output_payload_bytes,
        counts.publication_bytes,
    ])
}

fn checked_total_items(counts: ExactGeometryCounts) -> Result<usize, ()> {
    checked_sum([
        counts.owner_items,
        counts.input_items,
        counts.desired_target_items,
        counts.active_job_items,
        counts.pending_page_items,
        counts.pending_object_page_items,
        counts.scan_buffer_items,
        counts.active_atom_items,
        counts.deferred_object_items,
        counts.checkpoints,
        counts.continuation_items,
        counts.output_items,
        counts.publication_items,
    ])
}

fn checked_sum<const N: usize>(values: [usize; N]) -> Result<usize, ()> {
    values
        .into_iter()
        .try_fold(0usize, |total, value| total.checked_add(value).ok_or(()))
}

fn checked_add_counts(
    mut left: ExactGeometryCounts,
    right: ExactGeometryCounts,
) -> Result<ExactGeometryCounts, ()> {
    macro_rules! add {
        ($field:ident) => {
            left.$field = left.$field.checked_add(right.$field).ok_or(())?;
        };
    }
    add!(owner_bytes);
    add!(owner_items);
    add!(input_bytes);
    add!(input_items);
    add!(desired_target_bytes);
    add!(desired_target_items);
    add!(active_job_bytes);
    add!(active_job_items);
    add!(pending_page_bytes);
    add!(pending_page_items);
    add!(pending_object_page_bytes);
    add!(pending_object_page_items);
    add!(scan_buffer_bytes);
    add!(scan_buffer_items);
    add!(active_atom_bytes);
    add!(active_atom_items);
    add!(deferred_object_bytes);
    add!(deferred_object_items);
    add!(checkpoints);
    add!(checkpoint_bytes);
    add!(continuation_bytes);
    add!(continuation_items);
    add!(output_items);
    add!(output_record_bytes);
    add!(output_payload_bytes);
    add!(publication_bytes);
    add!(publication_items);
    Ok(left)
}
