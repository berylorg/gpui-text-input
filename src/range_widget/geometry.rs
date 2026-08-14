//! Exact geometry lifecycle and coherent-surface publication.

use gpui::{Context, Window};

use super::{
    CoherentRangeSurface, RangeScrollAnchor, RangeTextInput, RangeTextInputError,
    RangeTextInputRequest, SurfaceCandidate,
};
use crate::{
    ExactGeometryProgress, GeometryJobId, PageDemand, PageFailure, PageId, PagePurpose,
    PageRequest, PageRequestId, PageRequestKey, RangePage,
};

pub(super) struct PendingGeometryPage {
    job: crate::GeometryJobKey,
    request: PageRequest,
    wait: GeometryPageWait,
}

enum GeometryPageWait {
    Resident(PageId),
    Coalesced(PageRequestKey),
}

impl RangeTextInput {
    pub(super) fn start_index(&mut self) -> Result<(), RangeTextInputError> {
        let id = GeometryJobId::new(self.next_id());
        let start = self.geometry.start_index(id)?;
        self.release_geometry(start.release(), None, None);
        match start.progress() {
            ExactGeometryProgress::IndexComplete => self.start_or_resume_target(),
            ExactGeometryProgress::Scanning => {
                self.active_geometry = Some(start.key());
                self.request_geometry_page(start.key())
            }
            _ => Err(RangeTextInputError::Stale),
        }
    }

    pub(super) fn start_target(&mut self) -> Result<(), RangeTextInputError> {
        self.retire_surface_candidate();
        let id = GeometryJobId::new(self.next_id());
        let operation = self
            .geometry
            .request_block_target(id, self.desired.target())?;
        self.surface_candidate = Some(SurfaceCandidate {
            job: operation.key(),
            binding: self.config.binding,
            desired: self.desired,
            restoration: None,
        });
        self.accept_target_start(operation)
    }

    pub(super) fn start_restoration_target(
        &mut self,
        seed: crate::RangeRestorationSeed,
    ) -> Result<(), RangeTextInputError> {
        self.retire_surface_candidate();
        let id = GeometryJobId::new(self.next_id());
        let operation = self
            .geometry
            .request_block_target(id, self.desired.target())?;
        self.surface_candidate = Some(SurfaceCandidate {
            job: operation.key(),
            binding: self.config.binding,
            desired: self.desired,
            restoration: Some(seed),
        });
        self.accept_target_start(operation)
    }

    fn start_or_resume_target(&mut self) -> Result<(), RangeTextInputError> {
        let operation = if self.geometry.desired_target_key().is_some() {
            self.geometry.start_pending_target()?
        } else {
            self.retire_surface_candidate();
            let id = GeometryJobId::new(self.next_id());
            let operation = self
                .geometry
                .request_block_target(id, self.desired.target())?;
            self.surface_candidate = Some(SurfaceCandidate {
                job: operation.key(),
                binding: self.config.binding,
                desired: self.desired,
                restoration: None,
            });
            operation
        };
        self.accept_target_start(operation)
    }

    fn accept_target_start(
        &mut self,
        operation: crate::ExactGeometryStart,
    ) -> Result<(), RangeTextInputError> {
        self.release_geometry(operation.release(), None, None);
        match operation.progress() {
            ExactGeometryProgress::TargetComplete => self.publish_target(),
            ExactGeometryProgress::Scanning => {
                self.active_geometry = Some(operation.key());
                self.request_geometry_page(operation.key())
            }
            ExactGeometryProgress::PendingIndex => Ok(()),
            ExactGeometryProgress::IndexComplete => Err(RangeTextInputError::Stale),
        }
    }

    fn request_geometry_page(
        &mut self,
        job: crate::GeometryJobKey,
    ) -> Result<(), RangeTextInputError> {
        let id = PageRequestId::new(self.next_id());
        let request = self.geometry.request_page(job, id)?;
        let demand = self
            .residency
            .demand(id, request.key().purpose(), request.key().demand())
            .map_err(|_| {
                let _ = self.geometry.cancel(job);
                RangeTextInputError::Busy
            })?;
        match demand {
            PageDemand::Requested(expected) if expected.key() == request.key() => {
                self.requests
                    .push_back(RangeTextInputRequest::Page(request));
                Ok(())
            }
            PageDemand::ResidentAdjacent(page) => {
                self.pending_geometry_page = Some(PendingGeometryPage {
                    job,
                    request,
                    wait: GeometryPageWait::Resident(page),
                });
                Ok(())
            }
            PageDemand::Coalesced(existing) => {
                self.pending_geometry_page = Some(PendingGeometryPage {
                    job,
                    request,
                    wait: GeometryPageWait::Coalesced(existing),
                });
                Ok(())
            }
            _ => {
                let _ = self.residency.cancel(request.key());
                let _ = self.geometry.cancel(job);
                Err(RangeTextInputError::Stale)
            }
        }
    }

    pub(super) fn service_geometry_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let Some(mut pending) = self.pending_geometry_page.take() else {
            return Ok(());
        };
        if self.active_geometry != Some(pending.job) {
            return Err(RangeTextInputError::Stale);
        }
        if let GeometryPageWait::Coalesced(existing) = pending.wait {
            if self
                .residency
                .pending_requests()
                .any(|request| request == existing)
            {
                self.pending_geometry_page = Some(pending);
                return Ok(());
            }
            let demand = match self.residency.demand(
                pending.request.key().id(),
                pending.request.key().purpose(),
                pending.request.key().demand(),
            ) {
                Ok(demand) => demand,
                Err(_) => {
                    self.pending_geometry_page = Some(pending);
                    return Err(RangeTextInputError::Busy);
                }
            };
            pending.wait = match demand {
                PageDemand::ResidentAdjacent(page) => GeometryPageWait::Resident(page),
                PageDemand::Coalesced(request) => GeometryPageWait::Coalesced(request),
                PageDemand::Requested(request) if request.key() == pending.request.key() => {
                    self.requests
                        .push_back(RangeTextInputRequest::Page(request));
                    cx.notify();
                    return Ok(());
                }
                _ => return Err(RangeTextInputError::Stale),
            };
        }
        let GeometryPageWait::Resident(page_id) = pending.wait else {
            self.pending_geometry_page = Some(pending);
            return Ok(());
        };
        let admission = {
            let Some(page) = self.residency.page_by_id(page_id) else {
                if let Ok(release) = self.geometry.cancel(pending.job) {
                    self.release_geometry(&release, Some(pending.request.key()), Some(cx));
                }
                self.active_geometry = None;
                return Err(RangeTextInputError::Stale);
            };
            self.geometry
                .admit_resident_page(pending.job, page, window.text_system())
        };
        let admission = match admission {
            Ok(admission) => admission,
            Err(failure) => {
                let terminal = failure.release().jobs.contains(&pending.job);
                self.release_geometry(failure.release(), Some(pending.request.key()), Some(cx));
                if terminal {
                    self.active_geometry = None;
                } else {
                    if let Ok(release) = self.geometry.cancel(pending.job) {
                        self.release_geometry(&release, Some(pending.request.key()), Some(cx));
                    }
                    self.active_geometry = None;
                }
                return Err(RangeTextInputError::Geometry(failure.error().clone()));
            }
        };
        self.release_geometry(admission.release(), Some(pending.request.key()), Some(cx));
        self.advance_geometry(pending.job, admission.progress(), cx)
    }

    pub(super) fn geometry_waits_on(&self, key: PageRequestKey) -> bool {
        self.pending_geometry_page
            .as_ref()
            .is_some_and(|pending| matches!(pending.wait, GeometryPageWait::Coalesced(existing) if existing == key))
    }

    pub(super) fn deliver_geometry_page(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let job = self.active_geometry.ok_or(RangeTextInputError::Stale)?;
        let admission = match self.geometry.admit_page(job, &page, window.text_system()) {
            Ok(admission) => admission,
            Err(failure) => {
                let _ = self.residency.settle(page.key(), PageFailure::Malformed);
                let terminal = failure.release().jobs.contains(&job);
                self.release_geometry(failure.release(), Some(page.key()), Some(cx));
                if terminal {
                    self.active_geometry = None;
                }
                return Err(RangeTextInputError::Geometry(failure.error().clone()));
            }
        };
        let consumed = page.key();
        if consumed.purpose() == PagePurpose::GeometryTarget {
            self.residency
                .admit(page)
                .map_err(|_| RangeTextInputError::Stale)?;
        } else {
            let _ = self.residency.settle(consumed, PageFailure::Cancelled);
        }
        self.release_geometry(admission.release(), Some(consumed), Some(cx));
        self.advance_geometry(job, admission.progress(), cx)
    }

    fn advance_geometry(
        &mut self,
        job: crate::GeometryJobKey,
        progress: ExactGeometryProgress,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        match progress {
            ExactGeometryProgress::Scanning => self.request_geometry_page(job),
            ExactGeometryProgress::IndexComplete => {
                self.active_geometry = None;
                self.start_or_resume_target()
            }
            ExactGeometryProgress::TargetComplete => {
                self.active_geometry = None;
                self.publish_target()?;
                cx.notify();
                Ok(())
            }
            ExactGeometryProgress::PendingIndex => Err(RangeTextInputError::Stale),
        }
    }

    fn publish_target(&mut self) -> Result<(), RangeTextInputError> {
        let candidate_state = self
            .surface_candidate
            .take()
            .ok_or(RangeTextInputError::Stale)?;
        let index = self.geometry.index().ok_or(RangeTextInputError::Stale)?;
        let aggregate = index.aggregate();
        if candidate_state.binding != self.config.binding
            || candidate_state.job.geometry() != self.geometry.key()
        {
            return Err(RangeTextInputError::Stale);
        }
        let desired = candidate_state.desired;
        let required_anchor = if desired.preserve_scroll_anchor {
            Some(desired.scroll.source)
        } else if desired.reveal_caret {
            Some(desired.selection.head)
        } else {
            None
        };
        let target_ref = self.geometry.target().ok_or(RangeTextInputError::Stale)?;
        let retarget = if let Some(anchor) = required_anchor
            && (anchor < target_ref.predecessor() || anchor > target_ref.source_end())
        {
            if anchor < target_ref.predecessor() {
                Some(
                    index
                        .checkpoints()
                        .iter()
                        .rev()
                        .find(|checkpoint| checkpoint.source() <= anchor)
                        .map(|checkpoint| checkpoint.block_offset())
                        .ok_or(RangeTextInputError::IncompleteSurface)?,
                )
            } else {
                Some(
                    desired.target_block
                        + self
                            .desired
                            .viewport_extent
                            .max(self.config.layout.line_height),
                )
            }
        } else {
            None
        };
        let target = self
            .geometry
            .take_target()
            .ok_or(RangeTextInputError::Stale)?;
        if target.key() != candidate_state.job {
            return Err(RangeTextInputError::Stale);
        }
        let pages = self.residency.take_resident_pages();
        if let Some(target_block) = retarget {
            self.desired.target_block = target_block;
            drop(pages);
            drop(target);
            return self.start_target();
        }
        let candidate = CoherentRangeSurface::new(
            candidate_state.binding,
            pages,
            desired,
            target,
            aggregate.visual_lines(),
            aggregate.content_height(),
            self.config.layout.line_height,
            self.config.layout.wrap_width,
            self.config.placeholder.clone(),
        )?;
        let mut peak = self.surface.as_ref().map_or(candidate.charge(), |prior| {
            prior.charge().replacement_peak(candidate.charge())
        });
        peak.bytes = peak
            .bytes
            .checked_add(std::mem::size_of::<SurfaceCandidate>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        peak.items = peak
            .items
            .checked_add(1)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        if peak.bytes > self.config.limits.max_surface_bytes
            || peak.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        if let Some(seed) = candidate_state.restoration
            && (candidate.binding() != seed.binding
                || candidate.caret() != seed.caret
                || candidate.selection() != seed.selection
                || candidate.scroll_source() != seed.scroll.source
                || candidate.scroll_intra_anchor() != seed.scroll.intra_anchor
                || candidate.viewport() != seed.viewport
                || candidate.overscan() != seed.overscan)
        {
            return Err(RangeTextInputError::MalformedSeed);
        }
        self.last_surface_admission = Some(peak);
        self.surface = Some(candidate);
        if let Some(surface) = &self.surface {
            self.desired.scroll = RangeScrollAnchor {
                source: surface.scroll_source(),
                intra_anchor: surface.scroll_intra_anchor(),
            };
            self.desired.target_block = surface.scroll_block();
            self.desired.preserve_scroll_anchor = false;
            self.desired.reveal_caret = false;
        }
        Ok(())
    }

    pub(super) fn retire_surface_candidate(&mut self) {
        let Some(candidate) = self.surface_candidate.take() else {
            return;
        };
        if self.active_geometry == Some(candidate.job) {
            self.active_geometry = None;
        }
        if let Ok(release) = self.geometry.cancel(candidate.job) {
            self.release_geometry(&release, None, None);
        }
    }

    pub(super) fn release_geometry(
        &mut self,
        release: &crate::ExactGeometryRelease,
        completed_page: Option<crate::PageRequestKey>,
        mut cx: Option<&mut Context<Self>>,
    ) {
        if self.pending_geometry_page.as_ref().is_some_and(|pending| {
            release.jobs.contains(&pending.job) || release.pages.contains(&pending.request.key())
        }) {
            self.pending_geometry_page = None;
        }
        if self
            .surface_candidate
            .as_ref()
            .is_some_and(|candidate| release.jobs.contains(&candidate.job))
        {
            self.surface_candidate = None;
        }
        for page in &release.pages {
            if Some(*page) == completed_page {
                continue;
            }
            let _ = self.residency.cancel(*page);
            self.cancel_page_dispatch(*page);
        }
        if let Some(cx) = cx.as_mut() {
            cx.notify();
        }
    }

    pub(super) fn cancel_page_dispatch(&mut self, key: crate::PageRequestKey) {
        if let Some(index) = self.requests.iter().position(
            |request| matches!(request, RangeTextInputRequest::Page(page) if page.key() == key),
        ) {
            self.requests.remove(index);
        } else if self.dispatched_pages.remove(&key) {
            self.requests
                .push_back(RangeTextInputRequest::CancelPage(key));
        }
    }
}
