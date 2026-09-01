use std::sync::{Arc, Mutex, MutexGuard, Weak};

use gpui::WindowTextSystem;

use crate::{ObjectRequestKey, PageRequestKey, RangeSurfaceCharge};

use super::types::{
    RangePrepublicationFailure, RangePrepublicationSessionGeneration,
    RangePrepublicationValidationKey,
};

mod record;

use record::*;
pub(super) use record::{CleanupRegistrationError, CleanupRequest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangePrepublicationCleanupToken {
    slot: u32,
    id: u64,
}

impl RangePrepublicationCleanupToken {
    pub const fn id(self) -> u64 {
        self.id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangePrepublicationCleanupEffect {
    CancelValidation {
        token: RangePrepublicationCleanupToken,
        key: RangePrepublicationValidationKey,
    },
    ReleaseValidation {
        token: RangePrepublicationCleanupToken,
        key: RangePrepublicationValidationKey,
    },
    CancelPage {
        token: RangePrepublicationCleanupToken,
        generation: RangePrepublicationSessionGeneration,
        key: PageRequestKey,
    },
    ReleasePage {
        token: RangePrepublicationCleanupToken,
        generation: RangePrepublicationSessionGeneration,
        key: PageRequestKey,
    },
    CancelObjectPage {
        token: RangePrepublicationCleanupToken,
        generation: RangePrepublicationSessionGeneration,
        key: ObjectRequestKey,
    },
    ReleaseObjectPage {
        token: RangePrepublicationCleanupToken,
        generation: RangePrepublicationSessionGeneration,
        key: ObjectRequestKey,
    },
    ReleaseCandidate {
        token: RangePrepublicationCleanupToken,
        generation: RangePrepublicationSessionGeneration,
        environment_id: u64,
    },
}

impl RangePrepublicationCleanupEffect {
    pub const fn token(self) -> RangePrepublicationCleanupToken {
        match self {
            Self::CancelValidation { token, .. }
            | Self::ReleaseValidation { token, .. }
            | Self::CancelPage { token, .. }
            | Self::ReleasePage { token, .. }
            | Self::CancelObjectPage { token, .. }
            | Self::ReleaseObjectPage { token, .. }
            | Self::ReleaseCandidate { token, .. } => token,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangePrepublicationCleanupAcknowledgement {
    Accepted,
    Obsolete,
}

#[derive(Debug)]
pub struct RangePrepublicationCleanupStep {
    pub effects: Vec<RangePrepublicationCleanupEffect>,
    pub ready_remaining: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangePrepublicationCleanupOwnership {
    pub slots: usize,
    pub active: usize,
    pub ready: usize,
    pub awaiting_acknowledgement: usize,
    pub retained_charge: RangeSurfaceCharge,
}

#[derive(Clone)]
pub struct RangePrepublicationCleanupLedger {
    inner: Arc<Mutex<CleanupLedgerInner>>,
    text_system: Weak<WindowTextSystem>,
}

impl std::fmt::Debug for RangePrepublicationCleanupLedger {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RangePrepublicationCleanupLedger")
            .field("ownership", &self.ownership())
            .finish_non_exhaustive()
    }
}

impl RangePrepublicationCleanupLedger {
    pub fn new(
        text_system: &Arc<WindowTextSystem>,
        slots: usize,
    ) -> Result<Self, RangePrepublicationFailure> {
        if slots == 0 || u32::try_from(slots).is_err() {
            return Err(RangePrepublicationFailure::InvalidEnvironment);
        }
        let mut records = Vec::new();
        records
            .try_reserve_exact(slots)
            .map_err(|_| RangePrepublicationFailure::InitialCapacityDenied)?;
        records.resize_with(slots, CleanupSlot::default);
        Ok(Self {
            inner: Arc::new(Mutex::new(CleanupLedgerInner {
                records: records.into_boxed_slice(),
                next_id: 1,
            })),
            text_system: Arc::downgrade(text_system),
        })
    }

    pub fn service(&self, limit: usize) -> RangePrepublicationCleanupStep {
        let mut state = self.lock();
        let mut effects = Vec::with_capacity(limit.min(state.records.len()));
        if limit != 0 {
            for record in &mut state.records {
                if effects.len() == limit {
                    break;
                }
                let CleanupState::Ready(effect) = record.state else {
                    continue;
                };
                record.state = CleanupState::Draining {
                    effect,
                    followup: None,
                };
                effects.push(effect);
            }
        }
        let ready_remaining = state
            .records
            .iter()
            .filter(|record| matches!(record.state, CleanupState::Ready(_)))
            .count();
        RangePrepublicationCleanupStep {
            effects,
            ready_remaining,
        }
    }

    pub fn acknowledge(
        &self,
        token: RangePrepublicationCleanupToken,
    ) -> RangePrepublicationCleanupAcknowledgement {
        let mut state = self.lock();
        let Some(record) = record_mut(&mut state, token) else {
            return RangePrepublicationCleanupAcknowledgement::Obsolete;
        };
        let CleanupState::Draining { followup, .. } = record.state else {
            return RangePrepublicationCleanupAcknowledgement::Obsolete;
        };
        record.state = followup.map_or(CleanupState::Free, CleanupState::Ready);
        RangePrepublicationCleanupAcknowledgement::Accepted
    }

    pub fn ownership(&self) -> RangePrepublicationCleanupOwnership {
        let state = self.lock();
        let slots = state.records.len();
        let retained_charge = RangeSurfaceCharge {
            bytes: std::mem::size_of::<Mutex<CleanupLedgerInner>>()
                .saturating_add(slots.saturating_mul(std::mem::size_of::<CleanupSlot>())),
            items: 1usize.saturating_add(slots),
        };
        RangePrepublicationCleanupOwnership {
            slots,
            active: state
                .records
                .iter()
                .filter(|record| matches!(record.state, CleanupState::Active { .. }))
                .count(),
            ready: state
                .records
                .iter()
                .filter(|record| matches!(record.state, CleanupState::Ready(_)))
                .count(),
            awaiting_acknowledgement: state
                .records
                .iter()
                .filter(|record| matches!(record.state, CleanupState::Draining { .. }))
                .count(),
            retained_charge,
        }
    }

    pub(super) fn matches_text_system(&self, text_system: &Arc<WindowTextSystem>) -> bool {
        self.text_system
            .upgrade()
            .is_some_and(|current| Arc::ptr_eq(&current, text_system))
    }

    pub(super) const fn record_charge() -> RangeSurfaceCharge {
        RangeSurfaceCharge {
            bytes: std::mem::size_of::<CleanupSlot>(),
            items: 1,
        }
    }

    pub(super) fn reserve_request(
        &self,
        owner: RangePrepublicationSessionGeneration,
    ) -> Result<RangePrepublicationCleanupToken, CleanupRegistrationError> {
        let mut state = self.lock();
        let id = state.next_id;
        let Some(next_id) = id.checked_add(1) else {
            return Err(CleanupRegistrationError::Arithmetic);
        };
        let Some(slot) = state
            .records
            .iter()
            .position(|record| matches!(record.state, CleanupState::Free))
        else {
            return Err(CleanupRegistrationError::Full);
        };
        let token = RangePrepublicationCleanupToken {
            slot: u32::try_from(slot).map_err(|_| CleanupRegistrationError::Arithmetic)?,
            id,
        };
        state.next_id = next_id;
        state.records[slot] = CleanupSlot {
            id,
            owner: Some(CleanupOwner::Session(owner)),
            state: CleanupState::Active {
                request: CleanupRequest::Reserved,
                delivered: false,
            },
        };
        Ok(token)
    }

    pub(super) fn bind_request(
        &self,
        token: RangePrepublicationCleanupToken,
        request: CleanupRequest,
    ) -> bool {
        let mut state = self.lock();
        let Some(record) = record_mut(&mut state, token) else {
            return false;
        };
        let CleanupState::Active {
            request: current,
            delivered: false,
        } = &mut record.state
        else {
            return false;
        };
        if *current != CleanupRequest::Reserved {
            return false;
        }
        *current = request;
        true
    }

    pub(super) fn promote_candidate<I>(
        &self,
        token: RangePrepublicationCleanupToken,
        generation: RangePrepublicationSessionGeneration,
        environment_id: u64,
        resident: I,
    ) -> bool
    where
        I: Iterator<Item = RangePrepublicationCleanupToken> + Clone,
    {
        let mut state = self.lock();
        let Some(record) = record_ref(&state, token) else {
            return false;
        };
        if record.owner != Some(CleanupOwner::Session(generation))
            || !matches!(
                record.state,
                CleanupState::Active {
                    request: CleanupRequest::Reserved,
                    delivered: false
                }
            )
            || !resident.clone().all(|resident| {
                record_ref(&state, resident).is_some_and(|record| {
                    record.owner == Some(CleanupOwner::Session(generation))
                        && matches!(
                            record.state,
                            CleanupState::Active {
                                request: CleanupRequest::Page { .. }
                                    | CleanupRequest::ObjectPage { .. },
                                delivered: true
                            }
                        )
                })
            })
        {
            return false;
        }
        let record = record_mut(&mut state, token).expect("candidate record was validated");
        record.owner = Some(CleanupOwner::Candidate(generation));
        record.state = CleanupState::Active {
            request: CleanupRequest::Candidate {
                generation,
                environment_id,
            },
            delivered: true,
        };
        for resident in resident {
            record_mut(&mut state, resident)
                .expect("resident record was validated")
                .owner = Some(CleanupOwner::Candidate(generation));
        }
        true
    }

    pub(super) fn transfer_candidate_to_widget<I>(
        &self,
        candidate: RangePrepublicationCleanupToken,
        resident: I,
    ) -> bool
    where
        I: Iterator<Item = RangePrepublicationCleanupToken> + Clone,
    {
        let mut state = self.lock();
        let Some(candidate_record) = record_ref(&state, candidate) else {
            return false;
        };
        let Some(CleanupOwner::Candidate(generation)) = candidate_record.owner else {
            return false;
        };
        if !matches!(
            candidate_record.state,
            CleanupState::Active {
                request: CleanupRequest::Candidate { .. },
                delivered: true
            }
        ) || !resident.clone().all(|resident| {
            record_ref(&state, resident).is_some_and(|record| {
                record.owner == Some(CleanupOwner::Candidate(generation))
                    && matches!(
                        record.state,
                        CleanupState::Active {
                            request: CleanupRequest::Page { .. }
                                | CleanupRequest::ObjectPage { .. },
                            delivered: true
                        }
                    )
            })
        }) {
            return false;
        }
        let candidate_record =
            record_mut(&mut state, candidate).expect("candidate record was validated");
        candidate_record.state = CleanupState::Free;
        candidate_record.owner = None;
        for token in resident {
            record_mut(&mut state, token)
                .expect("resident record was validated")
                .owner = Some(CleanupOwner::Widget(generation));
        }
        true
    }

    pub(super) fn mark_delivered_request(
        &self,
        owner: RangePrepublicationSessionGeneration,
        request: CleanupRequest,
    ) -> Option<RangePrepublicationCleanupToken> {
        let mut state = self.lock();
        let (slot, record) = state.records.iter_mut().enumerate().find(|(_, record)| {
            record.owner == Some(CleanupOwner::Session(owner))
                && match record.state {
                    CleanupState::Active {
                        request: current, ..
                    } => current == request,
                    CleanupState::Ready(effect) | CleanupState::Draining { effect, .. } => {
                        effect_request(effect) == request
                    }
                    CleanupState::Free => false,
                }
        })?;
        let token = RangePrepublicationCleanupToken {
            slot: u32::try_from(slot).ok()?,
            id: record.id,
        };
        match &mut record.state {
            CleanupState::Active { delivered, .. } => *delivered = true,
            CleanupState::Ready(effect) => *effect = release_for(*effect),
            CleanupState::Draining {
                effect, followup, ..
            } => {
                if is_cancel_effect(*effect) && followup.is_none() {
                    *followup = Some(release_for(*effect));
                }
            }
            CleanupState::Free => return None,
        }
        Some(token)
    }

    pub(super) fn complete(&self, token: RangePrepublicationCleanupToken) -> bool {
        let mut state = self.lock();
        let Some(record) = record_mut(&mut state, token) else {
            return false;
        };
        if matches!(record.state, CleanupState::Active { .. }) {
            record.state = CleanupState::Free;
            record.owner = None;
            true
        } else {
            false
        }
    }

    pub(super) fn mark_token_ready(&self, token: RangePrepublicationCleanupToken) {
        let mut state = self.lock();
        let Some(record) = record_mut(&mut state, token) else {
            return;
        };
        mark_ready(record, token);
    }

    pub(super) fn mark_session_ready(&self, owner: RangePrepublicationSessionGeneration) {
        let mut state = self.lock();
        for (slot, record) in state.records.iter_mut().enumerate() {
            if record.owner != Some(CleanupOwner::Session(owner)) {
                continue;
            }
            let token = RangePrepublicationCleanupToken {
                slot: u32::try_from(slot).expect("ledger slot count was validated"),
                id: record.id,
            };
            mark_ready(record, token);
        }
    }

    pub(super) fn active_session_records(
        &self,
        owner: RangePrepublicationSessionGeneration,
    ) -> usize {
        self.lock()
            .records
            .iter()
            .filter(|record| {
                record.owner == Some(CleanupOwner::Session(owner))
                    && !matches!(record.state, CleanupState::Free)
            })
            .count()
    }

    fn lock(&self) -> MutexGuard<'_, CleanupLedgerInner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}

fn effect_request(effect: RangePrepublicationCleanupEffect) -> CleanupRequest {
    match effect {
        RangePrepublicationCleanupEffect::CancelValidation { key, .. }
        | RangePrepublicationCleanupEffect::ReleaseValidation { key, .. } => {
            CleanupRequest::Validation(key)
        }
        RangePrepublicationCleanupEffect::CancelPage {
            generation, key, ..
        }
        | RangePrepublicationCleanupEffect::ReleasePage {
            generation, key, ..
        } => CleanupRequest::Page { generation, key },
        RangePrepublicationCleanupEffect::CancelObjectPage {
            generation, key, ..
        }
        | RangePrepublicationCleanupEffect::ReleaseObjectPage {
            generation, key, ..
        } => CleanupRequest::ObjectPage { generation, key },
        RangePrepublicationCleanupEffect::ReleaseCandidate {
            generation,
            environment_id,
            ..
        } => CleanupRequest::Candidate {
            generation,
            environment_id,
        },
    }
}
