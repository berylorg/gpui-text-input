use crate::{ObjectRequestKey, PageRequestKey};

use super::{
    RangePrepublicationCleanupEffect, RangePrepublicationCleanupToken,
    RangePrepublicationSessionGeneration, RangePrepublicationValidationKey,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::range_widget::prepublication) enum CleanupRegistrationError {
    Full,
    Arithmetic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::range_widget::prepublication) enum CleanupRequest {
    Reserved,
    Validation(RangePrepublicationValidationKey),
    Page {
        generation: RangePrepublicationSessionGeneration,
        key: PageRequestKey,
    },
    ObjectPage {
        generation: RangePrepublicationSessionGeneration,
        key: ObjectRequestKey,
    },
    Candidate {
        generation: RangePrepublicationSessionGeneration,
        environment_id: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CleanupOwner {
    Session(RangePrepublicationSessionGeneration),
    Candidate(RangePrepublicationSessionGeneration),
    Widget(RangePrepublicationSessionGeneration),
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CleanupState {
    Free,
    Active {
        request: CleanupRequest,
        delivered: bool,
    },
    Ready(RangePrepublicationCleanupEffect),
    Draining {
        effect: RangePrepublicationCleanupEffect,
        followup: Option<RangePrepublicationCleanupEffect>,
    },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CleanupSlot {
    pub(super) id: u64,
    pub(super) owner: Option<CleanupOwner>,
    pub(super) state: CleanupState,
}

impl Default for CleanupSlot {
    fn default() -> Self {
        Self {
            id: 0,
            owner: None,
            state: CleanupState::Free,
        }
    }
}

pub(super) struct CleanupLedgerInner {
    pub(super) records: Box<[CleanupSlot]>,
    pub(super) next_id: u64,
}

pub(super) fn record_mut(
    state: &mut CleanupLedgerInner,
    token: RangePrepublicationCleanupToken,
) -> Option<&mut CleanupSlot> {
    let record = state.records.get_mut(usize::try_from(token.slot).ok()?)?;
    (record.id == token.id).then_some(record)
}

pub(super) fn record_ref(
    state: &CleanupLedgerInner,
    token: RangePrepublicationCleanupToken,
) -> Option<&CleanupSlot> {
    let record = state.records.get(usize::try_from(token.slot).ok()?)?;
    (record.id == token.id).then_some(record)
}

pub(super) fn mark_ready(record: &mut CleanupSlot, token: RangePrepublicationCleanupToken) {
    match record.state {
        CleanupState::Active {
            request: CleanupRequest::Reserved,
            ..
        } => {
            record.state = CleanupState::Free;
            record.owner = None;
        }
        CleanupState::Active { request, delivered } => {
            record.state = CleanupState::Ready(effect_for(token, request, delivered));
        }
        CleanupState::Ready(_) | CleanupState::Draining { .. } | CleanupState::Free => {}
    }
}

fn effect_for(
    token: RangePrepublicationCleanupToken,
    request: CleanupRequest,
    delivered: bool,
) -> RangePrepublicationCleanupEffect {
    match (request, delivered) {
        (CleanupRequest::Reserved, _) => unreachable!("reserved cleanup is never externally ready"),
        (CleanupRequest::Validation(key), false) => {
            RangePrepublicationCleanupEffect::CancelValidation { token, key }
        }
        (CleanupRequest::Validation(key), true) => {
            RangePrepublicationCleanupEffect::ReleaseValidation { token, key }
        }
        (CleanupRequest::Page { generation, key }, false) => {
            RangePrepublicationCleanupEffect::CancelPage {
                token,
                generation,
                key,
            }
        }
        (CleanupRequest::Page { generation, key }, true) => {
            RangePrepublicationCleanupEffect::ReleasePage {
                token,
                generation,
                key,
            }
        }
        (CleanupRequest::ObjectPage { generation, key }, false) => {
            RangePrepublicationCleanupEffect::CancelObjectPage {
                token,
                generation,
                key,
            }
        }
        (CleanupRequest::ObjectPage { generation, key }, true) => {
            RangePrepublicationCleanupEffect::ReleaseObjectPage {
                token,
                generation,
                key,
            }
        }
        (
            CleanupRequest::Candidate {
                generation,
                environment_id,
            },
            _,
        ) => RangePrepublicationCleanupEffect::ReleaseCandidate {
            token,
            generation,
            environment_id,
        },
    }
}

pub(super) fn release_for(
    effect: RangePrepublicationCleanupEffect,
) -> RangePrepublicationCleanupEffect {
    match effect {
        RangePrepublicationCleanupEffect::CancelValidation { token, key }
        | RangePrepublicationCleanupEffect::ReleaseValidation { token, key } => {
            RangePrepublicationCleanupEffect::ReleaseValidation { token, key }
        }
        RangePrepublicationCleanupEffect::CancelPage {
            token,
            generation,
            key,
        }
        | RangePrepublicationCleanupEffect::ReleasePage {
            token,
            generation,
            key,
        } => RangePrepublicationCleanupEffect::ReleasePage {
            token,
            generation,
            key,
        },
        RangePrepublicationCleanupEffect::CancelObjectPage {
            token,
            generation,
            key,
        }
        | RangePrepublicationCleanupEffect::ReleaseObjectPage {
            token,
            generation,
            key,
        } => RangePrepublicationCleanupEffect::ReleaseObjectPage {
            token,
            generation,
            key,
        },
        effect @ RangePrepublicationCleanupEffect::ReleaseCandidate { .. } => effect,
    }
}

pub(super) const fn is_cancel_effect(effect: RangePrepublicationCleanupEffect) -> bool {
    matches!(
        effect,
        RangePrepublicationCleanupEffect::CancelValidation { .. }
            | RangePrepublicationCleanupEffect::CancelPage { .. }
            | RangePrepublicationCleanupEffect::CancelObjectPage { .. }
    )
}
