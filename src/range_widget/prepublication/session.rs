use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use gpui::{Pixels, WindowTextSystem};

use crate::{
    BlockTarget, ExactGeometryError, ExactGeometryOwner, ExactGeometryProgress, GeometryJobId,
    GeometryJobKey, ObjectDemand, ObjectPage, ObjectPageAdmission, ObjectPageId, ObjectPurpose,
    ObjectRequestId, PageDemand, PageId, PagePurpose, PageRequestId, RangePage, RangeResidency,
    RangeSurfaceCharge, RangeTextInputError,
};

use super::super::restoration::{RestorationValidation, RestorationValidationNext};
use super::cleanup::{
    CleanupRegistrationError, CleanupRequest, RangePrepublicationCleanupLedger,
    RangePrepublicationCleanupToken,
};
use super::types::*;

static NEXT_SESSION_GENERATION: AtomicU64 = AtomicU64::new(1);

mod accounting;
mod candidate;
mod custody;
mod delivery;
mod progression;
mod startup;

pub use candidate::RangePrepublicationCandidate;
pub(in crate::range_widget) use custody::{ObjectCustody, TextCustody};
use delivery::DeliveredResponse;

enum SessionStage {
    Initializing,
    Validating,
    Restoration,
    Index,
    Target,
    Ready,
    Cancelled,
    Failed(RangePrepublicationFailure),
    CandidateTaken,
}

#[derive(Clone, Copy)]
enum Waiting {
    Validation(RangePrepublicationValidationRequest),
    RestorationPage {
        key: crate::PageRequestKey,
        cleanup: RangePrepublicationCleanupToken,
    },
    RestorationObject {
        key: crate::ObjectRequestKey,
        cleanup: RangePrepublicationCleanupToken,
    },
    GeometryPage {
        job: GeometryJobKey,
        key: crate::PageRequestKey,
        cleanup: RangePrepublicationCleanupToken,
    },
    GeometryObject {
        job: GeometryJobKey,
        key: crate::ObjectRequestKey,
        text_page: PageId,
        cleanup: RangePrepublicationCleanupToken,
    },
}

#[derive(Clone, Copy)]
struct AdmittedGeometryResponse {
    input: GeometryResponseInput,
    successor_ids: [u64; 3],
}

#[derive(Clone, Copy)]
enum GeometryResponseInput {
    Page {
        job: GeometryJobKey,
        page: PageId,
        resident: bool,
    },
    Object {
        job: GeometryJobKey,
        text_page: PageId,
        page: ObjectPageId,
        resident: bool,
    },
}

struct EffectBuffer {
    slots: [Option<RangePrepublicationEffect>; 2],
    len: usize,
}

impl EffectBuffer {
    const fn new() -> Self {
        Self {
            slots: [None, None],
            len: 0,
        }
    }

    const fn len(&self) -> usize {
        self.len
    }

    fn push(&mut self, effect: RangePrepublicationEffect) {
        assert!(self.len < self.slots.len());
        self.slots[self.len] = Some(effect);
        self.len += 1;
    }

    fn into_vec(mut self) -> Vec<RangePrepublicationEffect> {
        match self.len {
            0 => Vec::new(),
            1 => {
                let values: Box<[RangePrepublicationEffect]> =
                    Box::new([self.slots[0].take().expect("effect count is exact")]);
                values.into_vec()
            }
            2 => {
                let values: Box<[RangePrepublicationEffect]> = Box::new([
                    self.slots[0].take().expect("effect count is exact"),
                    self.slots[1].take().expect("effect count is exact"),
                ]);
                values.into_vec()
            }
            _ => unreachable!("effect buffer is bounded"),
        }
    }
}

pub struct RangePrepublicationSession {
    generation: RangePrepublicationSessionGeneration,
    environment: RangePrepublicationEnvironment,
    seed: crate::RangeRestorationSeed,
    stage: SessionStage,
    validation: RestorationValidation,
    accepted_validation: Option<RangePrepublicationValidationResponse>,
    residency: RangeResidency,
    object_residency: crate::ObjectResidency,
    geometry: Option<ExactGeometryOwner>,
    geometry_job: Option<GeometryJobKey>,
    waiting: Option<Waiting>,
    delivered: Option<DeliveredResponse>,
    admitted_geometry: Option<AdmittedGeometryResponse>,
    candidate: Option<RangePrepublicationCandidate>,
    text_custody: Vec<TextCustody>,
    object_custody: Vec<ObjectCustody>,
    next_id: u64,
    capacity_ceiling: RangeSurfaceCharge,
    available: RangeSurfaceCharge,
    high_water: RangeSurfaceCharge,
    ledger_blocked: bool,
}

impl RangePrepublicationSession {
    #[cfg(feature = "test-support")]
    pub(crate) fn initial_index_identity_for_test(&self) -> Option<u64> {
        (matches!(self.stage, SessionStage::Restoration)
            && self.waiting.is_none()
            && self.delivered.is_none()
            && matches!(self.validation.next(), RestorationValidationNext::Complete))
        .then_some(self.next_id)
    }

    #[cfg(feature = "test-support")]
    pub(crate) fn initial_index_peak_for_test(&self) -> Option<RangeSurfaceCharge> {
        let id = self.initial_index_identity_for_test()?;
        let charge = self.current_charge()?;
        let prepared = self
            .geometry
            .as_ref()?
            .prepare_initial_index(
                GeometryJobId::new(id),
                crate::range_geometry::ResponseCapacity::Enclosing {
                    current: (charge.bytes, charge.items),
                    limit: (usize::MAX, usize::MAX),
                },
            )
            .ok()?;
        let (bytes, items) = prepared.enclosing_peak();
        Some(RangeSurfaceCharge { bytes, items })
    }

    #[cfg(feature = "test-support")]
    pub(crate) fn response_successor_ids_for_test(&self) -> Option<[u64; 3]> {
        self.admitted_geometry
            .map(|response| response.successor_ids)
    }

    pub fn new(
        seed: crate::RangeRestorationSeed,
        environment: RangePrepublicationEnvironment,
    ) -> Result<Self, RangePrepublicationFailure> {
        let capacity = configured_capacity(environment.config());
        Self::new_with_admission_capacity(seed, environment, capacity)
    }

    pub fn new_with_admission_capacity(
        seed: crate::RangeRestorationSeed,
        environment: RangePrepublicationEnvironment,
        capacity: RangeSurfaceCharge,
    ) -> Result<Self, RangePrepublicationFailure> {
        validation_matches_seed(seed, environment.config().binding, seed.history)?;
        super::validate_seed(seed, environment.config())?;
        let generation_value = NEXT_SESSION_GENERATION
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| RangePrepublicationFailure::Arithmetic)?;
        let generation = RangePrepublicationSessionGeneration::new(generation_value);
        let config = environment.config();
        let configured = configured_capacity(config);
        let limit = RangeSurfaceCharge {
            bytes: capacity.bytes.min(configured.bytes),
            items: capacity.items.min(configured.items),
        };
        let (geometry, residency, object_residency) = startup::prepare(config, limit)?;
        let mut session = Self {
            generation,
            environment: environment.clone(),
            seed,
            stage: SessionStage::Initializing,
            validation: RestorationValidation::new(seed),
            accepted_validation: None,
            residency,
            object_residency,
            geometry: Some(geometry),
            geometry_job: None,
            waiting: None,
            delivered: None,
            admitted_geometry: None,
            candidate: None,
            text_custody: Vec::new(),
            object_custody: Vec::new(),
            next_id: 1,
            capacity_ceiling: limit,
            available: limit,
            high_water: RangeSurfaceCharge::default(),
            ledger_blocked: false,
        };
        let baseline = session
            .current_charge()
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
        let text_slots = config.residency_limits.max_resident_pages();
        let object_slots = config.object_residency_limits.max_resident_pages();
        let admit = |text, objects| {
            let storage = custody::storage_charge(text, objects)
                .ok_or(RangePrepublicationFailure::Arithmetic)?;
            let charge = add_charge(baseline, storage)?;
            if !charge_fits(charge, limit) {
                return Err(RangePrepublicationFailure::InitialCapacityDenied);
            }
            Ok(())
        };
        admit(text_slots, object_slots)?;
        session
            .text_custody
            .try_reserve_exact(text_slots)
            .map_err(|_| RangePrepublicationFailure::InitialCapacityDenied)?;
        admit(session.text_custody.capacity(), object_slots)?;
        session
            .object_custody
            .try_reserve_exact(object_slots)
            .map_err(|_| RangePrepublicationFailure::InitialCapacityDenied)?;
        let initial = session
            .current_charge()
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
        if !charge_fits(initial, limit) {
            return Err(RangePrepublicationFailure::InitialCapacityDenied);
        }
        session.high_water = initial;
        Ok(session)
    }

    pub const fn generation(&self) -> RangePrepublicationSessionGeneration {
        self.generation
    }

    pub fn status(&self) -> RangePrepublicationStatus {
        if self.ledger_blocked
            && !matches!(
                self.stage,
                SessionStage::Ready
                    | SessionStage::Cancelled
                    | SessionStage::Failed(_)
                    | SessionStage::CandidateTaken
            )
        {
            return RangePrepublicationStatus::CapacityBlocked;
        }
        match self.stage {
            SessionStage::Initializing => RangePrepublicationStatus::Initializing,
            SessionStage::Validating => {
                if self.delivered.is_some() {
                    RangePrepublicationStatus::Advancing
                } else {
                    RangePrepublicationStatus::Validating
                }
            }
            SessionStage::Restoration | SessionStage::Index | SessionStage::Target => {
                if self.delivered.is_some() {
                    let charge = self.response_coexistence_charge();
                    if charge.is_some_and(|charge| !charge_fits(charge, self.available)) {
                        RangePrepublicationStatus::CapacityBlocked
                    } else {
                        RangePrepublicationStatus::Advancing
                    }
                } else if self.admitted_geometry.is_some() {
                    if self
                        .current_charge()
                        .is_some_and(|charge| !charge_fits(charge, self.available))
                    {
                        RangePrepublicationStatus::CapacityBlocked
                    } else {
                        RangePrepublicationStatus::Advancing
                    }
                } else if self.waiting.is_some() {
                    RangePrepublicationStatus::WaitingForResponse
                } else {
                    RangePrepublicationStatus::Advancing
                }
            }
            SessionStage::Ready => RangePrepublicationStatus::Ready,
            SessionStage::Cancelled => RangePrepublicationStatus::Cancelled,
            SessionStage::Failed(failure) => RangePrepublicationStatus::Failed(failure),
            SessionStage::CandidateTaken => RangePrepublicationStatus::Stale,
        }
    }

    pub fn set_available_capacity(&mut self, available: RangeSurfaceCharge) {
        self.available = RangeSurfaceCharge {
            bytes: available.bytes.min(self.capacity_ceiling.bytes),
            items: available.items.min(self.capacity_ceiling.items),
        };
    }

    pub fn ownership(&self) -> RangePrepublicationOwnership {
        let charge = self.current_charge().unwrap_or(RangeSurfaceCharge {
            bytes: usize::MAX,
            items: usize::MAX,
        });
        let pages = self.residency.counts();
        let objects = self.object_residency.counts();
        RangePrepublicationOwnership {
            bytes: charge.bytes,
            items: charge.items,
            resident_pages: pages.resident_pages,
            resident_object_pages: objects.resident_pages,
            pending_pages: pages.pending_requests,
            pending_object_pages: objects.pending_requests,
            candidate: self.candidate.is_some(),
        }
    }

    pub const fn high_water(&self) -> RangeSurfaceCharge {
        self.high_water
    }

    pub fn service(
        &mut self,
        text_system: &Arc<WindowTextSystem>,
    ) -> RangePrepublicationServiceStep {
        if !self.environment.matches_text_system(text_system) {
            self.fail_without_effects(RangePrepublicationFailure::Stale);
            return RangePrepublicationServiceStep {
                status: self.status(),
                spent: 0,
                effects: Vec::new(),
            };
        }
        self.ledger_blocked = false;
        let limit = self
            .environment
            .config()
            .limits
            .max_realization_work_per_frame;
        let mut spent = 0usize;
        let mut effects = EffectBuffer::new();
        while spent < limit && effects.len() < 2 {
            match self.advance_one(text_system, &mut effects) {
                Ok(true) => spent += 1,
                Ok(false) => break,
                Err(failure) => {
                    self.fail(failure, &mut effects);
                    break;
                }
            }
            if self.waiting.is_some()
                && self.delivered.is_none()
                && self.admitted_geometry.is_none()
            {
                break;
            }
        }
        self.observe_high_water();
        RangePrepublicationServiceStep {
            status: self.status(),
            spent,
            effects: effects.into_vec(),
        }
    }

    pub fn cancel(&mut self) {
        self.environment
            .cleanup()
            .mark_session_ready(self.generation);
        self.release_all_resident_custody();
        self.cancel_waiting();
        self.delivered = None;
        self.admitted_geometry = None;
        self.candidate = None;
        if let Some(geometry) = self.geometry.as_mut() {
            let _ = geometry.dispose();
        }
        let _ = self.residency.dispose();
        let _ = self.object_residency.dispose();
        self.geometry_job = None;
        self.stage = SessionStage::Cancelled;
    }

    pub fn take_candidate(&mut self) -> Option<RangePrepublicationCandidate> {
        if !matches!(self.stage, SessionStage::Ready) {
            return None;
        }
        let candidate = self.candidate.take()?;
        self.stage = SessionStage::CandidateTaken;
        Some(candidate)
    }
}

impl Drop for RangePrepublicationSession {
    fn drop(&mut self) {
        self.environment
            .cleanup()
            .mark_session_ready(self.generation);
    }
}

fn configured_capacity(config: &crate::RangeTextInputConfig) -> RangeSurfaceCharge {
    RangeSurfaceCharge {
        bytes: config.limits.max_surface_bytes,
        items: config.limits.max_surface_items,
    }
}

fn add_charge(
    left: RangeSurfaceCharge,
    right: RangeSurfaceCharge,
) -> Result<RangeSurfaceCharge, RangePrepublicationFailure> {
    Ok(RangeSurfaceCharge {
        bytes: left
            .bytes
            .checked_add(right.bytes)
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
        items: left
            .items
            .checked_add(right.items)
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
    })
}

fn nested_owner_charge(
    charge: RangeSurfaceCharge,
    inline_bytes: usize,
) -> Option<RangeSurfaceCharge> {
    Some(RangeSurfaceCharge {
        bytes: charge.bytes.checked_sub(inline_bytes)?,
        items: charge.items.checked_sub(1)?,
    })
}

fn multiply_charge(charge: RangeSurfaceCharge, count: usize) -> Option<RangeSurfaceCharge> {
    Some(RangeSurfaceCharge {
        bytes: charge.bytes.checked_mul(count)?,
        items: charge.items.checked_mul(count)?,
    })
}

fn object_page_id(admission: ObjectPageAdmission) -> ObjectPageId {
    match admission {
        ObjectPageAdmission::Admitted { page, .. }
        | ObjectPageAdmission::Reconciled { page, .. } => page,
    }
}

fn classify_geometry_error(error: ExactGeometryError) -> RangePrepublicationFailure {
    match error {
        ExactGeometryError::CapacityExceeded => RangePrepublicationFailure::TerminalCapacity,
        ExactGeometryError::InvalidLimits | ExactGeometryError::InvalidMetric => {
            RangePrepublicationFailure::InvalidEnvironment
        }
        ExactGeometryError::SourceContract
        | ExactGeometryError::WrongPage(_)
        | ExactGeometryError::WrongObjectPage(_)
        | ExactGeometryError::NoncontiguousPage { .. }
        | ExactGeometryError::PageTooLarge => RangePrepublicationFailure::MalformedResponse,
        _ => RangePrepublicationFailure::DeterministicGeometry,
    }
}

fn classify_widget_error(error: RangeTextInputError) -> RangePrepublicationFailure {
    match error {
        RangeTextInputError::MalformedSeed => RangePrepublicationFailure::MalformedResponse,
        RangeTextInputError::SurfaceCapacity | RangeTextInputError::DetachedCapacity => {
            RangePrepublicationFailure::TerminalCapacity
        }
        RangeTextInputError::Stale => RangePrepublicationFailure::Stale,
        RangeTextInputError::Geometry(error) => classify_geometry_error(error),
        _ => RangePrepublicationFailure::DeterministicGeometry,
    }
}

fn classify_page_admission(error: crate::PageAdmissionError) -> RangePrepublicationFailure {
    match error {
        crate::PageAdmissionError::LimitExceeded(_) => RangePrepublicationFailure::TerminalCapacity,
        crate::PageAdmissionError::Malformed(_) => RangePrepublicationFailure::MalformedResponse,
        crate::PageAdmissionError::Stale(_)
        | crate::PageAdmissionError::Cancelled(_)
        | crate::PageAdmissionError::Unavailable(_) => RangePrepublicationFailure::Stale,
    }
}

fn classify_object_admission(error: crate::ObjectPageAdmissionError) -> RangePrepublicationFailure {
    match error {
        crate::ObjectPageAdmissionError::LimitExceeded(_) => {
            RangePrepublicationFailure::TerminalCapacity
        }
        crate::ObjectPageAdmissionError::Malformed(_) => {
            RangePrepublicationFailure::MalformedResponse
        }
        crate::ObjectPageAdmissionError::Stale(_)
        | crate::ObjectPageAdmissionError::Cancelled(_)
        | crate::ObjectPageAdmissionError::Unavailable(_) => RangePrepublicationFailure::Stale,
    }
}
