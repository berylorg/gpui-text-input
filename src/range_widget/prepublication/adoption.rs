use gpui::{Context, Window};

use super::super::{AdoptedPrepublicationOwners, RangeTextInput};
use super::types::*;

impl RangeTextInput {
    pub fn new_with_prepublication(
        environment: &RangePrepublicationEnvironment,
        mut candidate: super::RangePrepublicationCandidate,
        current: RangePrepublicationCurrent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, RangePrepublicationAdoptionError> {
        if !environment.matches_candidate(&candidate.environment)
            || environment.id() != candidate.environment_id
            || !environment.matches_text_system(window.text_system())
            || !candidate.text_system.upgrade().is_some_and(|text_system| {
                std::sync::Arc::ptr_eq(&text_system, window.text_system())
            })
        {
            return Err(RangePrepublicationAdoptionError::EnvironmentMismatch);
        }
        super::validate_seed(candidate.seed, environment.config())
            .map_err(|_| RangePrepublicationAdoptionError::SourceMismatch)?;
        if current.binding != candidate.seed.binding
            || candidate.validation.binding != current.binding
            || !candidate.validation.current
        {
            return Err(RangePrepublicationAdoptionError::SourceMismatch);
        }
        if current.history != candidate.seed.history
            || candidate.validation.history != current.history
        {
            return Err(RangePrepublicationAdoptionError::HistoryMismatch);
        }
        let configured = crate::RangeSurfaceCharge {
            bytes: environment.config().limits.max_surface_bytes,
            items: environment.config().limits.max_surface_items,
        };
        if !charge_fits(current.available_capacity, configured)
            || !charge_fits(candidate.adoption_peak, current.available_capacity)
        {
            return Err(RangePrepublicationAdoptionError::CapacityMismatch);
        }
        let seed = candidate.seed;
        let surface_charge = candidate.surface_charge;
        let next_id = candidate.next_id;
        let geometry = candidate
            .geometry
            .take()
            .ok_or(RangePrepublicationAdoptionError::CandidateConsumed)?;
        let surface = candidate
            .surface
            .take()
            .ok_or(RangePrepublicationAdoptionError::CandidateConsumed)?;
        let mut input = Self::from_prepublication_owners(
            environment.config().clone(),
            AdoptedPrepublicationOwners {
                geometry,
                surface,
                seed,
                surface_charge,
                history: current.history,
                next_id,
            },
            window,
            cx,
        )
        .map_err(|_| RangePrepublicationAdoptionError::WidgetConstruction)?;
        let custody = candidate
            .take_adopted_cleanup()
            .ok_or(RangePrepublicationAdoptionError::CandidateConsumed)?;
        input.install_adopted_prepublication_custody(custody);
        Ok(input)
    }
}
