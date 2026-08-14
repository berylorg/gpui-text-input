use std::{mem::size_of, sync::Arc};

use gpui::WindowTextSystem;

use crate::{
    ByteOffset, PageDemandEnvelope, PageDirection, PageEdgeFact, PageRequestKey, RangePage,
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
        self.admit_page_inner(key, page, text_system, false)
    }

    pub(crate) fn admit_resident_page(
        &mut self,
        key: GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        self.admit_page_inner(key, page, text_system, true)
    }

    fn admit_page_inner(
        &mut self,
        key: GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
        resident: bool,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        let Some(mut active) = self.active.take() else {
            return Err(self.nonterminal_failure(ExactGeometryError::ObsoleteJob(key)));
        };
        if active.key != key {
            self.active = Some(active);
            return Err(self.nonterminal_failure(ExactGeometryError::ObsoleteJob(key)));
        }
        let Some(expected) = active.pending.as_deref().copied() else {
            self.active = Some(active);
            return Err(self.nonterminal_failure(ExactGeometryError::NoActiveJob));
        };
        if (!resident && page.key() != expected)
            || (resident && !resident_page_satisfies(page, expected))
        {
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
        let page_scan = match scan::process_page(
            &mut active,
            page,
            text_system,
            &inputs.layout,
            &inputs.style,
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
        if let scan::PageScan::NeedContext {
            required_end,
            replay,
        } = page_scan
        {
            return context::defer(self, active, expected, required_end, replay, budget);
        }
        let reached_source_end = page.range().end().get() == source_end;
        let target_ready = match active.kind {
            ActiveKind::Target { target, .. } => {
                checkpoint::target_scan_ready(&active.scanner, target)
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
                return Err(self.terminal_failure(error, stage, active, &budget));
            }
            if matches!(active.kind, ActiveKind::Index) {
                let terminal =
                    match checkpoint::make_checkpoint(&active.scanner, &inputs.layout, true) {
                        Ok(terminal) => terminal,
                        Err(error) => {
                            return Err(self.terminal_failure(
                                error,
                                ExactGeometryFailureStage::Checkpoint,
                                active,
                                &budget,
                            ));
                        }
                    };
                if let Err(error) = budget.observe(&active, size_of::<ExactGeometryCheckpoint>(), 0)
                {
                    return Err(self.terminal_failure(
                        error,
                        ExactGeometryFailureStage::Checkpoint,
                        active,
                        &budget,
                    ));
                }
                checkpoint::retain_checkpoint(
                    &mut active.scanner.checkpoints,
                    terminal,
                    self.limits.max_checkpoints,
                );
            }
        }
        active.page_use = ActivePageUse::Traverse {
            anchor: page.range().end(),
        };
        if !reached_source_end && !target_ready {
            active.pending = None;
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
                ExactGeometryProgress::Scanning,
                consumed_page_release(expected),
                &budget,
            ));
        }
        self.publish_candidate(active, expected, budget)
    }

    fn publish_candidate(
        &mut self,
        active: Box<ActiveJob>,
        page: PageRequestKey,
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
            return Err(self.terminal_failure(
                error,
                ExactGeometryFailureStage::Publication,
                active,
                &budget,
            ));
        }
        let active_release_counts = accounting::active_counts(&active);
        let completion_counts = completion_release_counts(&active);
        let ActiveJob {
            key, kind, scanner, ..
        } = *active;
        match kind {
            ActiveKind::Index => {
                let candidate = ExactGeometryIndex {
                    key,
                    aggregate: ExactGeometryAggregate {
                        visual_lines: scanner.continuation.visual_lines,
                        content_height: scanner.continuation.block_offset,
                    },
                    checkpoints: Arc::from(scanner.checkpoints.into_iter().collect::<Vec<_>>()),
                };
                let retained = accounting::counts_with_index_candidate(self, &candidate);
                if retained.total_bytes() > self.limits.max_retained_bytes
                    || retained.total_items() > self.limits.max_retained_items
                {
                    return Err(candidate_failure(
                        ExactGeometryError::CapacityExceeded,
                        key,
                        page,
                        active_release_counts,
                        &budget,
                    ));
                }
                let prior = self.index.replace(Box::new(candidate));
                let release = merge_release(
                    merge_release(
                        consumed_page_release(page),
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
            ActiveKind::Target { predecessor, .. } => {
                let target_source = scanner.target_source.unwrap_or(scanner.target_line_source);
                let candidate = BlockTargetPublication {
                    key,
                    predecessor,
                    target_source: ByteOffset::new(target_source),
                    source_end: ByteOffset::new(scanner.continuation.next_logical_offset),
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
                        page,
                        active_release_counts,
                        &budget,
                    ));
                }
                let prior = self.target.replace(Box::new(candidate));
                let release = merge_release(
                    merge_release(
                        consumed_page_release(page),
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
        let page = active.pending.as_deref().copied().into_iter().collect();
        ExactGeometryFailure {
            error,
            stage,
            release: ExactGeometryRelease {
                jobs: vec![active.key],
                pages: page,
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

fn candidate_failure(
    error: ExactGeometryError,
    key: GeometryJobKey,
    page: PageRequestKey,
    counts: ExactGeometryCounts,
    budget: &AdmissionBudget,
) -> ExactGeometryFailure {
    ExactGeometryFailure {
        error,
        stage: ExactGeometryFailureStage::Publication,
        release: ExactGeometryRelease {
            jobs: vec![key],
            pages: vec![page],
            counts,
        },
        admission_required_bytes: budget.peak_bytes,
        admission_required_items: budget.peak_items,
    }
}
