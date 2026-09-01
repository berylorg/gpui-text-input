use std::sync::{Arc, Weak};

use gpui::WindowTextSystem;

use crate::{
    ObjectRequest, PageRequest, RangeBinding, RangeHistoryFrontier, RangeRestorationSeed,
    RangeSurfaceCharge, RangeTextInputConfig,
};

#[derive(Clone)]
pub struct RangePrepublicationEnvironment {
    pub(super) inner: Arc<RangePrepublicationEnvironmentInner>,
}

pub(super) struct RangePrepublicationEnvironmentInner {
    pub id: u64,
    pub config: RangeTextInputConfig,
    pub text_system: Weak<WindowTextSystem>,
    pub cleanup: super::RangePrepublicationCleanupLedger,
}

impl RangePrepublicationEnvironment {
    pub fn new(
        id: u64,
        config: RangeTextInputConfig,
        text_system: &Arc<WindowTextSystem>,
        cleanup: super::RangePrepublicationCleanupLedger,
    ) -> Result<Self, RangePrepublicationFailure> {
        super::validate_environment(&config)?;
        if !cleanup.matches_text_system(text_system) {
            return Err(RangePrepublicationFailure::InvalidEnvironment);
        }
        Ok(Self {
            inner: Arc::new(RangePrepublicationEnvironmentInner {
                id,
                config,
                text_system: Arc::downgrade(text_system),
                cleanup,
            }),
        })
    }

    pub fn id(&self) -> u64 {
        self.inner.id
    }

    pub fn config(&self) -> &RangeTextInputConfig {
        &self.inner.config
    }

    pub fn cleanup(&self) -> &super::RangePrepublicationCleanupLedger {
        &self.inner.cleanup
    }

    pub(super) fn matches_text_system(&self, text_system: &Arc<WindowTextSystem>) -> bool {
        self.inner
            .text_system
            .upgrade()
            .is_some_and(|current| Arc::ptr_eq(&current, text_system))
    }

    pub(super) fn matches_candidate(
        &self,
        candidate: &std::sync::Weak<RangePrepublicationEnvironmentInner>,
    ) -> bool {
        std::sync::Weak::ptr_eq(&Arc::downgrade(&self.inner), candidate)
    }
}

impl std::fmt::Debug for RangePrepublicationEnvironment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RangePrepublicationEnvironment")
            .field("id", &self.id())
            .field("binding", &self.config().binding)
            .field(
                "presentation_generation",
                &self.config().presentation_generation,
            )
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangePrepublicationSessionGeneration(u64);

impl RangePrepublicationSessionGeneration {
    pub const fn get(self) -> u64 {
        self.0
    }

    pub(super) const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangePrepublicationValidationKey {
    pub generation: RangePrepublicationSessionGeneration,
    pub request: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangePrepublicationValidationRequest {
    pub cleanup: super::RangePrepublicationCleanupToken,
    pub key: RangePrepublicationValidationKey,
    pub binding: RangeBinding,
    pub history: Option<RangeHistoryFrontier>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangePrepublicationValidationResponse {
    pub key: RangePrepublicationValidationKey,
    pub binding: RangeBinding,
    pub history: Option<RangeHistoryFrontier>,
    pub current: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangePrepublicationCurrent {
    pub binding: RangeBinding,
    pub history: Option<RangeHistoryFrontier>,
    pub available_capacity: RangeSurfaceCharge,
}

#[derive(Debug)]
pub enum RangePrepublicationEffect {
    ValidateOwner(RangePrepublicationValidationRequest),
    Page {
        cleanup: super::RangePrepublicationCleanupToken,
        generation: RangePrepublicationSessionGeneration,
        request: PageRequest,
    },
    ObjectPage {
        cleanup: super::RangePrepublicationCleanupToken,
        generation: RangePrepublicationSessionGeneration,
        request: ObjectRequest,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangePrepublicationFailure {
    InvalidEnvironment,
    InitialCapacityDenied,
    SourceMismatch,
    HistoryMismatch,
    Stale,
    ExactKeyCollision,
    MalformedResponse,
    DeterministicGeometry,
    TerminalCapacity,
    Arithmetic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangePrepublicationStatus {
    Initializing,
    Validating,
    WaitingForResponse,
    Advancing,
    CapacityBlocked,
    Ready,
    Cancelled,
    Stale,
    Failed(RangePrepublicationFailure),
}

#[derive(Debug)]
pub struct RangePrepublicationServiceStep {
    pub status: RangePrepublicationStatus,
    pub spent: usize,
    pub effects: Vec<RangePrepublicationEffect>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangePrepublicationDelivery {
    Accepted,
    CapacityBlocked,
    Obsolete,
    Terminal(RangePrepublicationFailure),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangePrepublicationOwnership {
    pub bytes: usize,
    pub items: usize,
    pub resident_pages: usize,
    pub resident_object_pages: usize,
    pub pending_pages: usize,
    pub pending_object_pages: usize,
    pub candidate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangePrepublicationAdoptionError {
    EnvironmentMismatch,
    SourceMismatch,
    HistoryMismatch,
    CapacityMismatch,
    CandidateConsumed,
    WidgetConstruction,
}

pub(super) fn validation_matches_seed(
    seed: RangeRestorationSeed,
    binding: RangeBinding,
    history: Option<RangeHistoryFrontier>,
) -> Result<(), RangePrepublicationFailure> {
    if seed.binding != binding {
        return Err(RangePrepublicationFailure::SourceMismatch);
    }
    if seed.history != history {
        return Err(RangePrepublicationFailure::HistoryMismatch);
    }
    Ok(())
}

pub(super) fn charge_fits(charge: RangeSurfaceCharge, limit: RangeSurfaceCharge) -> bool {
    charge.bytes <= limit.bytes && charge.items <= limit.items
}
