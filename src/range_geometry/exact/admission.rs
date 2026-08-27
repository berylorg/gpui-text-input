use std::{mem::size_of, sync::Arc};

use gpui::WindowTextSystem;

use crate::{
    ByteOffset, InlineObjectGap, ObjectPage, ObjectRequestKey, PageEdgeFact, PageRequestKey,
    RangePage, RangeSourceSelection, SourcePosition,
};

use super::*;

mod context;
mod release;

pub(super) use release::{index_release, merge_release, target_release};

impl ExactGeometryOwner {
    pub fn admit_page(
        &mut self,
        key: GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        self.admit_page_inner(key, page, text_system)
    }

    fn admit_page_inner(
        &mut self,
        key: GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        let Some(mut active) = self.active.take() else {
            return Err(self.nonterminal_failure(ExactGeometryError::ObsoleteJob(key)));
        };
        if active.key != key {
            self.active = Some(active);
            return Err(self.nonterminal_failure(ExactGeometryError::ObsoleteJob(key)));
        }
        let Some(PendingInput::Text(expected)) = active.pending.as_deref().copied() else {
            self.active = Some(active);
            return Err(self.nonterminal_failure(ExactGeometryError::NoActiveJob));
        };
        if page.key() != expected {
            self.active = Some(active);
            return Err(self.nonterminal_failure(ExactGeometryError::WrongPage(page.key())));
        }

        let fixed = accounting::fixed_counts_without_active(self);
        let mut budget = AdmissionBudget {
            fixed_bytes: fixed.total_bytes(),
            fixed_items: fixed.total_items(),
            page_payload_bytes: page.retained_charge().bytes(),
            page_items: page.retained_charge().items(),
            max_bytes: self.limits.max_retained_bytes,
            max_items: self.limits.max_retained_items,
            peak_bytes: 0,
            peak_items: 0,
            failure_stage: None,
        };
        if let Err(error) = budget.observe(&active, 0, 0) {
            return Err(self.terminal_failure(
                error,
                ExactGeometryFailureStage::PageCoexistence,
                active,
                &budget,
            ));
        }
        if page.range().len() > self.limits.max_page_bytes {
            return Err(self.terminal_failure(
                ExactGeometryError::CapacityExceeded,
                ExactGeometryFailureStage::PageCoexistence,
                active,
                &budget,
            ));
        }
        let window_identity = text_system as *const WindowTextSystem as usize;
        if active
            .window_identity
            .is_some_and(|identity| identity != window_identity)
        {
            return Err(self.terminal_failure(
                ExactGeometryError::SourceContract,
                ExactGeometryFailureStage::WindowIdentity,
                active,
                &budget,
            ));
        }
        active.window_identity = Some(window_identity);
        let inputs = self.inputs.as_deref().expect("active owner retains inputs");
        let source_end = inputs.binding.extent().byte_len();
        let requested_edge_matches = match active.page_use {
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
            return Err(self.terminal_failure(
                ExactGeometryError::SourceContract,
                ExactGeometryFailureStage::PageCoexistence,
                active,
                &budget,
            ));
        }
        if let ActivePageUse::Context { replay, .. } = active.page_use {
            return context::admit(self, active, page, expected, replay, budget);
        }
        active.pending = None;
        active.text_page = Some(ActiveTextPage {
            id: page.id(),
            range: page.range(),
        });
        if let Err(error) = budget.observe(&active, 0, 0) {
            return Err(self.terminal_failure(
                error,
                ExactGeometryFailureStage::PageCoexistence,
                active,
                &budget,
            ));
        }
        self.high_water_bytes = self.high_water_bytes.max(budget.peak_bytes);
        self.high_water_items = self.high_water_items.max(budget.peak_items);
        self.active = Some(active);
        return Ok(self.page_admission(
            ExactGeometryProgress::NeedObjects,
            consumed_page_release(expected),
            &budget,
        ));
    }

    pub fn admit_object_page(
        &mut self,
        key: GeometryJobKey,
        text_page: &RangePage,
        object_page: &ObjectPage,
        text_system: &WindowTextSystem,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        let Some(mut active) = self.active.take() else {
            return Err(self.nonterminal_failure(ExactGeometryError::ObsoleteJob(key)));
        };
        if active.key != key {
            self.active = Some(active);
            return Err(self.nonterminal_failure(ExactGeometryError::ObsoleteJob(key)));
        }
        let Some(PendingInput::Object(expected)) = active.pending.as_deref().copied() else {
            self.active = Some(active);
            return Err(self.nonterminal_failure(ExactGeometryError::WrongInputKind));
        };
        let Some(active_page) = active.text_page else {
            self.active = Some(active);
            return Err(self.nonterminal_failure(ExactGeometryError::WrongInputKind));
        };
        if text_page.id() != active_page.id
            || text_page.range() != active_page.range
            || !resident_object_page_satisfies(object_page, expected)
        {
            self.active = Some(active);
            return Err(
                self.nonterminal_failure(ExactGeometryError::WrongObjectPage(object_page.key()))
            );
        }
        let fixed = accounting::fixed_counts_without_active(self);
        let mut budget = AdmissionBudget {
            fixed_bytes: fixed.total_bytes(),
            fixed_items: fixed.total_items(),
            page_payload_bytes: text_page
                .retained_charge()
                .bytes()
                .saturating_add(object_page.retained_charge().bytes()),
            page_items: text_page
                .retained_charge()
                .items()
                .saturating_add(object_page.objects().len())
                .saturating_add(1),
            max_bytes: self.limits.max_retained_bytes,
            max_items: self.limits.max_retained_items,
            peak_bytes: 0,
            peak_items: 0,
            failure_stage: None,
        };
        if let Err(error) = budget.observe(&active, 0, 0) {
            return Err(self.terminal_failure(
                error,
                ExactGeometryFailureStage::PageCoexistence,
                active,
                &budget,
            ));
        }
        let inputs = self.inputs.as_deref().expect("active owner retains inputs");
        let source_end = inputs.binding.extent().byte_len();
        let scan = match scan::process_object_page(
            &mut active,
            text_page,
            object_page,
            text_system,
            inputs,
            self.limits,
            source_end,
            &mut budget,
        ) {
            Ok(scan) => scan,
            Err(error) => {
                let stage = budget
                    .failure_stage
                    .unwrap_or(ExactGeometryFailureStage::Scan);
                return Err(self.terminal_failure(error, stage, active, &budget));
            }
        };
        active.pending = None;
        if let scan::PageScan::NeedContext {
            required_end,
            replay,
        } = scan
        {
            return context::defer(self, active, expected, required_end, replay, budget);
        }
        let release = consumed_object_release(expected);
        let target_ready = match active.kind {
            ActiveKind::Target { target, anchor, .. } => {
                anchor.is_some_and(|anchor| {
                    matches!(
                        anchor.gap,
                        crate::InlineObjectGap::Before(_) | crate::InlineObjectGap::Between { .. }
                    )
                }) && checkpoint::target_scan_ready(&active.scanner, target, anchor)
            }
            ActiveKind::Index => false,
        };
        if target_ready {
            active.scanner.deferred_object = None;
            active.text_page = None;
            return self.publish_candidate(active, release, budget);
        }
        if !object_page.complete() {
            self.high_water_bytes = self.high_water_bytes.max(budget.peak_bytes);
            self.high_water_items = self.high_water_items.max(budget.peak_items);
            self.active = Some(active);
            return Ok(self.page_admission(ExactGeometryProgress::NeedObjects, release, &budget));
        }
        active.text_page = None;
        self.finish_text_page(
            active,
            active_page.range.end(),
            release,
            text_system,
            budget,
        )
    }

    fn finish_text_page(
        &mut self,
        mut active: Box<ActiveJob>,
        page_end: ByteOffset,
        release: ExactGeometryRelease,
        text_system: &WindowTextSystem,
        mut budget: AdmissionBudget,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        let inputs = self.inputs.as_deref().expect("active owner retains inputs");
        let source_end = inputs.binding.extent().byte_len();
        let reached_source_end = page_end.get() == source_end;
        let target_ready = match active.kind {
            ActiveKind::Target { target, anchor, .. } => {
                checkpoint::target_scan_ready(&active.scanner, target, anchor)
            }
            ActiveKind::Index => false,
        };
        if reached_source_end {
            if let Err(error) = scan::finalize_source(
                &mut active,
                text_system,
                &inputs.layout,
                &inputs.style,
                self.limits,
                source_end,
                &mut budget,
            ) {
                let stage = budget
                    .failure_stage
                    .unwrap_or(ExactGeometryFailureStage::Finalize);
                let mut failure = self.terminal_failure(error, stage, active, &budget);
                failure.release = merge_release(release, failure.release);
                return Err(failure);
            }
            if matches!(active.kind, ActiveKind::Index) {
                let terminal =
                    match checkpoint::make_checkpoint(&active.scanner, &inputs.layout, true) {
                        Ok(terminal) => terminal,
                        Err(error) => {
                            let mut failure = self.terminal_failure(
                                error,
                                ExactGeometryFailureStage::Checkpoint,
                                active,
                                &budget,
                            );
                            failure.release = merge_release(release, failure.release);
                            return Err(failure);
                        }
                    };
                if let Err(error) = budget.observe(&active, size_of::<ExactGeometryCheckpoint>(), 1)
                {
                    let mut failure = self.terminal_failure(
                        error,
                        ExactGeometryFailureStage::Checkpoint,
                        active,
                        &budget,
                    );
                    failure.release = merge_release(release, failure.release);
                    return Err(failure);
                }
                checkpoint::retain_checkpoint(
                    &mut active.scanner.checkpoints,
                    terminal,
                    self.limits.max_checkpoints,
                );
            }
        }
        active.page_use = ActivePageUse::Traverse { anchor: page_end };
        if !reached_source_end && !target_ready {
            if let Err(error) = budget.observe(&active, 0, 0) {
                let mut failure = self.terminal_failure(
                    error,
                    ExactGeometryFailureStage::PageCoexistence,
                    active,
                    &budget,
                );
                failure.release = merge_release(release, failure.release);
                return Err(failure);
            }
            self.high_water_bytes = self.high_water_bytes.max(budget.peak_bytes);
            self.high_water_items = self.high_water_items.max(budget.peak_items);
            self.active = Some(active);
            return Ok(self.page_admission(ExactGeometryProgress::Scanning, release, &budget));
        }
        self.publish_candidate(active, release, budget)
    }

    fn publish_candidate(
        &mut self,
        active: Box<ActiveJob>,
        completion_release: ExactGeometryRelease,
        mut budget: AdmissionBudget,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        let (conversion_bytes, conversion_items) = match active.kind {
            ActiveKind::Index => (
                accounting::index_publication_record_bytes(active.scanner.checkpoints.len()),
                1usize.saturating_add(active.scanner.checkpoints.len()),
            ),
            ActiveKind::Target { .. } => (
                accounting::target_publication_record_bytes(active.scanner.fragments.len()),
                1usize.saturating_add(active.scanner.fragments.len()),
            ),
        };
        // Vec/VecDeque storage remains live while Arc publication records are initialized, so the
        // conversion peak includes both record sets. Payload behind fragment Arcs is not cloned.
        if let Err(error) = budget.observe(&active, conversion_bytes, conversion_items) {
            let mut failure = self.terminal_failure(
                error,
                ExactGeometryFailureStage::Publication,
                active,
                &budget,
            );
            failure.release = merge_release(completion_release, failure.release);
            return Err(failure);
        }
        let active_release_counts = accounting::active_counts(&active);
        let completion_counts = completion_release_counts(&active);
        let ActiveJob {
            key, kind, scanner, ..
        } = *active;
        match kind {
            ActiveKind::Index => {
                let extent = self
                    .inputs
                    .as_deref()
                    .expect("active owner retains inputs")
                    .binding
                    .extent()
                    .byte_len();
                let document_selection = exact_document_selection(&scanner, extent);
                let candidate = ExactGeometryIndex {
                    key,
                    aggregate: ExactGeometryAggregate {
                        visual_lines: scanner.continuation.visual_lines,
                        content_height: scanner.continuation.block_offset,
                    },
                    checkpoints: Arc::from(scanner.checkpoints.into_iter().collect::<Vec<_>>()),
                    document_selection,
                };
                let retained = accounting::counts_with_index_candidate(self, &candidate);
                if retained.total_bytes() > self.limits.max_retained_bytes
                    || retained.total_items() > self.limits.max_retained_items
                {
                    return Err(candidate_failure(
                        ExactGeometryError::CapacityExceeded,
                        key,
                        completion_release,
                        active_release_counts,
                        &budget,
                    ));
                }
                let prior = self.index.replace(Box::new(candidate));
                let release = merge_release(
                    merge_release(
                        completion_release,
                        ExactGeometryRelease {
                            counts: completion_counts,
                            ..Default::default()
                        },
                    ),
                    prior.map_or_else(ExactGeometryRelease::default, index_release),
                );
                self.high_water_bytes = self.high_water_bytes.max(budget.peak_bytes);
                self.high_water_items = self.high_water_items.max(budget.peak_items);
                self.observe_current();
                Ok(self.page_admission(ExactGeometryProgress::IndexComplete, release, &budget))
            }
            ActiveKind::Target {
                predecessor,
                predecessor_checkpoint,
                ..
            } => {
                let target_source = scanner
                    .target_source
                    .unwrap_or(scanner.target_line_position);
                let candidate = BlockTargetPublication {
                    key,
                    predecessor,
                    target_source,
                    source_end: scanner
                        .continuation
                        .next_position
                        .try_into()
                        .expect("accepted GPUI position is source-compatible"),
                    predecessor_checkpoint,
                    visual_lines_lower_bound: scanner.continuation.visual_lines,
                    content_height_lower_bound: scanner.continuation.block_offset
                        + scanner.continuation.line_block_extent,
                    fragments: Arc::from(scanner.fragments),
                    charge: scanner.output_charge,
                    item_charge: scanner.output_item_charge,
                };
                let retained = accounting::counts_with_target_candidate(self, &candidate);
                if retained.total_bytes() > self.limits.max_retained_bytes
                    || retained.total_items() > self.limits.max_retained_items
                {
                    return Err(candidate_failure(
                        ExactGeometryError::CapacityExceeded,
                        key,
                        completion_release,
                        active_release_counts,
                        &budget,
                    ));
                }
                let prior = self.target.replace(Box::new(candidate));
                let release = merge_release(
                    merge_release(
                        completion_release,
                        ExactGeometryRelease {
                            counts: completion_counts,
                            ..Default::default()
                        },
                    ),
                    prior.map_or_else(ExactGeometryRelease::default, target_release),
                );
                self.high_water_bytes = self.high_water_bytes.max(budget.peak_bytes);
                self.high_water_items = self.high_water_items.max(budget.peak_items);
                self.observe_current();
                Ok(self.page_admission(ExactGeometryProgress::TargetComplete, release, &budget))
            }
        }
    }

    fn page_admission(
        &self,
        progress: ExactGeometryProgress,
        release: ExactGeometryRelease,
        budget: &AdmissionBudget,
    ) -> ExactGeometryAdmission {
        ExactGeometryAdmission {
            progress,
            release,
            admission_required_bytes: budget.peak_bytes,
            admission_required_items: budget.peak_items,
        }
    }

    fn nonterminal_failure(&self, error: ExactGeometryError) -> ExactGeometryFailure {
        ExactGeometryFailure {
            error,
            stage: ExactGeometryFailureStage::Validation,
            release: ExactGeometryRelease::default(),
            admission_required_bytes: self.counts().total_bytes(),
            admission_required_items: self.counts().total_items(),
        }
    }

    fn terminal_failure(
        &self,
        error: ExactGeometryError,
        stage: ExactGeometryFailureStage,
        active: Box<ActiveJob>,
        budget: &AdmissionBudget,
    ) -> ExactGeometryFailure {
        let counts = accounting::active_counts(&active);
        let required_bytes = budget
            .fixed_bytes
            .saturating_add(budget.page_payload_bytes)
            .saturating_add(counts.total_bytes());
        let required_items = budget
            .fixed_items
            .saturating_add(budget.page_items)
            .saturating_add(counts.total_items());
        let mut pages = Vec::new();
        let mut object_pages = Vec::new();
        match active.pending.as_deref().copied() {
            Some(PendingInput::Text(page)) => pages.push(page),
            Some(PendingInput::Object(page)) => object_pages.push(page),
            None => {}
        }
        ExactGeometryFailure {
            error,
            stage,
            release: ExactGeometryRelease {
                jobs: vec![active.key],
                pages,
                object_pages,
                counts,
            },
            admission_required_bytes: budget.peak_bytes.max(required_bytes),
            admission_required_items: budget.peak_items.max(required_items),
        }
    }
}

fn consumed_page_release(page: PageRequestKey) -> ExactGeometryRelease {
    // RangePage payload is borrowed: it contributes to admission peak but never enters owner
    // counts. The page key releases host-side page ownership; only the pending key is owner-held.
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

fn completion_release_counts(active: &ActiveJob) -> ExactGeometryCounts {
    let mut counts = accounting::active_counts(active);
    counts.pending_page_bytes = 0;
    counts.pending_page_items = 0;
    match active.kind {
        ActiveKind::Index => {
            counts.checkpoints = 0;
            counts.checkpoint_bytes = 0;
        }
        ActiveKind::Target { .. } => {
            counts.output_items = 0;
            counts.output_record_bytes = 0;
            counts.output_payload_bytes = 0;
        }
    }
    counts
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

fn candidate_failure(
    error: ExactGeometryError,
    key: GeometryJobKey,
    mut release: ExactGeometryRelease,
    counts: ExactGeometryCounts,
    budget: &AdmissionBudget,
) -> ExactGeometryFailure {
    ExactGeometryFailure {
        error,
        stage: ExactGeometryFailureStage::Publication,
        release: {
            release.jobs.push(key);
            release.counts = counts;
            release
        },
        admission_required_bytes: budget.peak_bytes,
        admission_required_items: budget.peak_items,
    }
}
