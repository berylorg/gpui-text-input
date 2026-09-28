mod highlight_geometry;
use std::sync::Weak;

mod fragment_maps;
mod page_order;
mod realized_buffers;
mod transfer;

use gpui::WindowTextSystem;

use crate::{ExactGeometryOwner, ObjectPage, RangePage, RangeSurfaceCharge};

use super::super::super::{CoherentRangeSurface, DesiredSurface, RangeScrollAnchor};
use super::super::{cleanup::*, types::*};
use super::{
    ObjectCustody, RangePrepublicationSession, SessionStage, TextCustody, add_charge,
    classify_widget_error, configured_capacity, multiply_charge, nested_owner_charge,
};

pub struct RangePrepublicationCandidate {
    pub(in crate::range_widget::prepublication) environment:
        Weak<RangePrepublicationEnvironmentInner>,
    pub(in crate::range_widget::prepublication) environment_id: u64,
    pub(in crate::range_widget::prepublication) text_system: Weak<WindowTextSystem>,
    pub(in crate::range_widget::prepublication) cleanup_ledger: RangePrepublicationCleanupLedger,
    pub(in crate::range_widget::prepublication) cleanup: Option<RangePrepublicationCleanupToken>,
    text_custody: Vec<TextCustody>,
    object_custody: Vec<ObjectCustody>,
    pub(in crate::range_widget::prepublication) generation: RangePrepublicationSessionGeneration,
    pub(in crate::range_widget::prepublication) seed: crate::RangeRestorationSeed,
    pub(in crate::range_widget::prepublication) validation: RangePrepublicationValidationResponse,
    pub(in crate::range_widget::prepublication) geometry: Option<ExactGeometryOwner>,
    pub(in crate::range_widget::prepublication) surface: Option<CoherentRangeSurface>,
    pub(in crate::range_widget::prepublication) surface_charge: RangeSurfaceCharge,
    pub(in crate::range_widget::prepublication) charge: RangeSurfaceCharge,
    pub(in crate::range_widget::prepublication) adoption_peak: RangeSurfaceCharge,
    pub(in crate::range_widget::prepublication) next_id: u64,
    pub(in crate::range_widget::prepublication) origin_session_charge: RangeSurfaceCharge,
}

impl std::fmt::Debug for RangePrepublicationCandidate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RangePrepublicationCandidate")
            .field("environment_id", &self.environment_id)
            .field("seed", &self.seed)
            .field("charge", &self.charge)
            .field("adoption_peak", &self.adoption_peak)
            .finish_non_exhaustive()
    }
}

impl RangePrepublicationCandidate {
    pub const fn source_binding(&self) -> crate::RangeBinding {
        self.seed.binding
    }

    pub const fn history(&self) -> Option<crate::RangeHistoryFrontier> {
        self.seed.history
    }

    pub const fn retained_charge(&self) -> RangeSurfaceCharge {
        self.charge
    }

    pub const fn adoption_peak(&self) -> RangeSurfaceCharge {
        self.adoption_peak
    }

    pub fn environment_id(&self) -> u64 {
        self.environment_id
    }

    pub const fn generation(&self) -> RangePrepublicationSessionGeneration {
        self.generation
    }

    pub(in crate::range_widget::prepublication) fn take_adopted_cleanup(
        &mut self,
    ) -> Option<super::super::AdoptedPrepublicationCustody> {
        let Some(cleanup) = self.cleanup else {
            return None;
        };
        let resident = self
            .text_custody
            .iter()
            .map(|custody| custody.cleanup)
            .chain(self.object_custody.iter().map(|custody| custody.cleanup));
        if !self
            .cleanup_ledger
            .transfer_candidate_to_widget(cleanup, resident)
        {
            return None;
        }
        self.cleanup = None;
        Some(super::super::AdoptedPrepublicationCustody::new(
            self.cleanup_ledger.clone(),
            std::mem::take(&mut self.text_custody),
            std::mem::take(&mut self.object_custody),
        ))
    }
}

impl Drop for RangePrepublicationCandidate {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            self.cleanup_ledger.mark_token_ready(cleanup);
        }
        for custody in self.text_custody.drain(..) {
            self.cleanup_ledger.mark_token_ready(custody.cleanup);
        }
        for custody in self.object_custody.drain(..) {
            self.cleanup_ledger.mark_token_ready(custody.cleanup);
        }
    }
}

impl RangePrepublicationSession {
    #[cfg(feature = "test-support")]
    pub fn test_prepare_transfer_buffers(
        text: usize,
        objects: usize,
        current: RangeSurfaceCharge,
        configured: RangeSurfaceCharge,
        available: RangeSurfaceCharge,
        peak: &mut RangeSurfaceCharge,
    ) -> Result<Option<(Vec<RangePage>, Vec<ObjectPage>)>, RangePrepublicationFailure> {
        transfer::prepare(text, objects, current, configured, available, peak)
    }

    pub(super) fn finish_candidate(&mut self) -> Result<(), RangePrepublicationFailure> {
        let Some(cleanup) = self.reserve_cleanup()? else {
            return Ok(());
        };
        let config = self.environment.config().clone();
        super::super::validate_seed(self.seed, &config)?;
        let current_charge = self
            .current_charge()
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
        let mut transfer_peak = current_charge;
        let transfer = transfer::prepare(
            self.residency.counts().resident_pages,
            self.object_residency.counts().resident_pages,
            current_charge,
            configured_capacity(&config),
            self.available,
            &mut transfer_peak,
        );
        self.observe_charge(transfer_peak);
        let Some((pages, object_pages)) = transfer? else {
            let _ = self.environment.cleanup().complete(cleanup);
            self.ledger_blocked = true;
            return Ok(());
        };
        let surface_baseline = add_charge(
            current_charge,
            add_charge(
                transfer::charge(pages.capacity(), object_pages.capacity())?,
                RangeSurfaceCharge {
                    bytes: std::mem::size_of::<
                        crate::range_widget::surface::PreparedCoherentRangeSurface,
                    >(),
                    items: 1,
                },
            )?,
        )?;
        let mut surface_peak = surface_baseline;
        let page_order = page_order::prepare(
            self.residency
                .resident_page_iter()
                .map(|page| page.range().start()),
            surface_baseline,
            configured_capacity(&config),
            self.available,
            &mut surface_peak,
        );
        self.observe_charge(surface_peak);
        let Some(page_order) = page_order? else {
            let _ = self.environment.cleanup().complete(cleanup);
            self.ledger_blocked = true;
            return Ok(());
        };
        let map_baseline = add_charge(
            surface_baseline,
            RangeSurfaceCharge {
                bytes: page_order
                    .len()
                    .checked_mul(std::mem::size_of::<
                        crate::range_widget::surface::SurfacePageIndex,
                    >())
                    .ok_or(RangePrepublicationFailure::Arithmetic)?,
                items: page_order.len(),
            },
        )?;
        let fragments = self
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.target())
            .ok_or(RangePrepublicationFailure::DeterministicGeometry)?
            .fragments();
        let owned_maps = fragment_maps::prepare(
            crate::range_widget::surface::fragment_maps::owned_maps(fragments),
            map_baseline,
            configured_capacity(&config),
            self.available,
            &mut surface_peak,
        );
        self.observe_charge(surface_peak);
        let Some(owned_maps) = owned_maps? else {
            let _ = self.environment.cleanup().complete(cleanup);
            self.ledger_blocked = true;
            return Ok(());
        };
        let realized_baseline = add_charge(
            map_baseline,
            RangeSurfaceCharge {
                bytes: owned_maps
                    .capacity()
                    .checked_mul(std::mem::size_of::<gpui::StreamingLayoutMap>())
                    .ok_or(RangePrepublicationFailure::Arithmetic)?,
                items: owned_maps.capacity(),
            },
        )?;
        let fragments = self
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.target())
            .ok_or(RangePrepublicationFailure::DeterministicGeometry)?
            .fragments();
        let (objects, gaps) =
            crate::range_widget::surface::realized_buffers::counts(fragments, &owned_maps);
        let buffers = realized_buffers::prepare(
            objects,
            gaps,
            realized_baseline,
            configured_capacity(&config),
            self.available,
            &mut surface_peak,
        );
        self.observe_charge(surface_peak);
        let Some(buffers) = buffers? else {
            let _ = self.environment.cleanup().complete(cleanup);
            self.ledger_blocked = true;
            return Ok(());
        };
        let geometry = self
            .geometry
            .as_ref()
            .ok_or(RangePrepublicationFailure::Stale)?;
        let index = geometry
            .index()
            .ok_or(RangePrepublicationFailure::DeterministicGeometry)?;
        let target = geometry
            .target()
            .ok_or(RangePrepublicationFailure::DeterministicGeometry)?;
        let aggregate = index.aggregate();
        let mut desired = DesiredSurface::origin(
            config.viewport_extent,
            super::super::super::bounded_realization_extent(
                config.viewport_extent,
                config.limits.max_realized_block_extent,
            ),
            config.overscan,
        );
        desired.source_selection = Some(self.seed.selection);
        desired.scroll = RangeScrollAnchor {
            source: self.seed.scroll.position.byte_offset,
            intra_anchor: self.seed.scroll.intra_anchor,
        };
        desired.preserve_scroll_anchor = true;
        desired.reveal_caret = false;
        let prepared = highlight_geometry::prepare(
            realized_baseline,
            configured_capacity(&config),
            self.available,
            &mut surface_peak,
            |admit| {
                CoherentRangeSurface::prepare(
                    self.seed.binding,
                    self.residency.resident_page_iter(),
                    self.object_residency.resident_page_iter(),
                    desired,
                    Some((self.seed.caret, self.seed.selection)),
                    Some(self.seed.scroll.position),
                    target,
                    aggregate.quality(),
                    aggregate.visual_lines(),
                    aggregate.content_height(),
                    config.layout.line_height,
                    config.layout.wrap_width,
                    config.placeholder.clone(),
                    page_order,
                    owned_maps,
                    buffers,
                    admit,
                )
            },
        );
        self.observe_charge(surface_peak);
        let Some(prepared) = prepared? else {
            let _ = self.environment.cleanup().complete(cleanup);
            self.ledger_blocked = true;
            return Ok(());
        };
        let preparation_allocation = [
            RangeSurfaceCharge {
                bytes: std::mem::size_of_val(&prepared),
                items: 1,
            },
            prepared.retained_charge(),
            RangeSurfaceCharge {
                bytes: pages
                    .capacity()
                    .checked_mul(std::mem::size_of::<RangePage>())
                    .ok_or(RangePrepublicationFailure::Arithmetic)?,
                items: pages.capacity(),
            },
            RangeSurfaceCharge {
                bytes: object_pages
                    .capacity()
                    .checked_mul(std::mem::size_of::<ObjectPage>())
                    .ok_or(RangePrepublicationFailure::Arithmetic)?,
                items: object_pages.capacity(),
            },
        ]
        .into_iter()
        .try_fold(RangeSurfaceCharge::default(), |total, charge| {
            add_charge(total, charge).ok()
        })
        .ok_or(RangePrepublicationFailure::Arithmetic)?;
        let preparation_peak = add_charge(current_charge, preparation_allocation)?;
        self.observe_charge(preparation_peak);
        let configured = configured_capacity(&config);
        if !charge_fits(preparation_peak, configured) {
            return Err(RangePrepublicationFailure::TerminalCapacity);
        }
        if !charge_fits(preparation_peak, self.available) {
            let _ = self.environment.cleanup().complete(cleanup);
            self.ledger_blocked = true;
            return Ok(());
        }
        let pages = self.residency.take_resident_pages_into(pages);
        let object_pages = self.object_residency.take_resident_pages_into(object_pages);
        let geometry = self
            .geometry
            .as_mut()
            .ok_or(RangePrepublicationFailure::Stale)?;
        let target = geometry
            .take_target()
            .ok_or(RangePrepublicationFailure::DeterministicGeometry)?;
        let geometry_charge = geometry.counts();
        let presentation_overlap = geometry
            .presentation_overlap_bytes(object_pages.iter())
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
        let unused_surface_slots = RangeSurfaceCharge {
            bytes: pages
                .capacity()
                .checked_sub(pages.len())
                .and_then(|count| count.checked_mul(std::mem::size_of::<RangePage>()))
                .and_then(|bytes| {
                    object_pages
                        .capacity()
                        .checked_sub(object_pages.len())
                        .and_then(|count| count.checked_mul(std::mem::size_of::<ObjectPage>()))
                        .and_then(|object_bytes| bytes.checked_add(object_bytes))
                })
                .ok_or(RangePrepublicationFailure::Arithmetic)?,
            items: pages
                .capacity()
                .checked_sub(pages.len())
                .and_then(|items| {
                    object_pages
                        .capacity()
                        .checked_sub(object_pages.len())
                        .and_then(|object_items| items.checked_add(object_items))
                })
                .ok_or(RangePrepublicationFailure::Arithmetic)?,
        };
        let surface_charge = add_charge(prepared.charge(), unused_surface_slots)?;
        let surface = CoherentRangeSurface::commit_prepared(prepared, pages, object_pages, target);
        let candidate_charge = [
            RangeSurfaceCharge {
                bytes: std::mem::size_of::<RangePrepublicationCandidate>(),
                items: 1,
            },
            nested_owner_charge(surface_charge, std::mem::size_of::<CoherentRangeSurface>())
                .ok_or(RangePrepublicationFailure::Arithmetic)?,
            nested_owner_charge(
                RangeSurfaceCharge {
                    bytes: geometry_charge
                        .total_bytes()
                        .checked_sub(presentation_overlap)
                        .ok_or(RangePrepublicationFailure::Arithmetic)?,
                    items: geometry_charge.total_items(),
                },
                std::mem::size_of::<ExactGeometryOwner>(),
            )
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
            self.custody_storage_charge()
                .ok_or(RangePrepublicationFailure::Arithmetic)?,
            multiply_charge(
                RangePrepublicationCleanupLedger::record_charge(),
                self.text_custody
                    .len()
                    .checked_add(self.object_custody.len())
                    .and_then(|records| records.checked_add(1))
                    .ok_or(RangePrepublicationFailure::Arithmetic)?,
            )
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
        ]
        .into_iter()
        .try_fold(RangeSurfaceCharge::default(), |total, charge| {
            add_charge(total, charge).ok()
        })
        .ok_or(RangePrepublicationFailure::Arithmetic)?;
        if !charge_fits(candidate_charge, configured) {
            return Err(RangePrepublicationFailure::TerminalCapacity);
        }
        let validation = self
            .accepted_validation
            .ok_or(RangePrepublicationFailure::Stale)?;
        let geometry = self
            .geometry
            .take()
            .ok_or(RangePrepublicationFailure::Stale)?;
        let resident_cleanup = self
            .text_custody
            .iter()
            .map(|custody| custody.cleanup)
            .chain(self.object_custody.iter().map(|custody| custody.cleanup));
        if !self.environment.cleanup().promote_candidate(
            cleanup,
            self.generation,
            self.environment.id(),
            resident_cleanup,
        ) {
            return Err(RangePrepublicationFailure::Stale);
        }
        let text_custody = std::mem::take(&mut self.text_custody);
        let object_custody = std::mem::take(&mut self.object_custody);
        self.candidate = Some(RangePrepublicationCandidate {
            environment: std::sync::Arc::downgrade(&self.environment.inner),
            environment_id: self.environment.id(),
            text_system: self.environment.inner.text_system.clone(),
            cleanup_ledger: self.environment.cleanup().clone(),
            cleanup: Some(cleanup),
            text_custody,
            object_custody,
            generation: self.generation,
            seed: self.seed,
            validation,
            geometry: Some(geometry),
            surface: Some(surface),
            surface_charge,
            charge: candidate_charge,
            adoption_peak: RangeSurfaceCharge::default(),
            next_id: self.next_id,
            origin_session_charge: RangeSurfaceCharge::default(),
        });
        self.geometry_job = None;
        let ready_charge = self
            .current_charge()
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
        if !charge_fits(ready_charge, configured) {
            self.candidate = None;
            return Err(RangePrepublicationFailure::TerminalCapacity);
        }
        let nested_candidate = nested_owner_charge(
            candidate_charge,
            std::mem::size_of::<RangePrepublicationCandidate>(),
        )
        .ok_or(RangePrepublicationFailure::Arithmetic)?;
        let origin_session_charge = RangeSurfaceCharge {
            bytes: ready_charge
                .bytes
                .checked_sub(nested_candidate.bytes)
                .ok_or(RangePrepublicationFailure::Arithmetic)?,
            items: ready_charge
                .items
                .checked_sub(nested_candidate.items)
                .ok_or(RangePrepublicationFailure::Arithmetic)?,
        };
        let adoption_peak = add_charge(
            add_charge(candidate_charge, origin_session_charge)?,
            super::super::adoption_widget_support_charge(&config)?,
        )?;
        if !charge_fits(adoption_peak, configured) {
            self.candidate = None;
            return Err(RangePrepublicationFailure::TerminalCapacity);
        }
        let candidate = self
            .candidate
            .as_mut()
            .ok_or(RangePrepublicationFailure::Stale)?;
        candidate.origin_session_charge = origin_session_charge;
        candidate.adoption_peak = adoption_peak;
        self.stage = SessionStage::Ready;
        Ok(())
    }
}
