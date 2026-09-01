mod adopted_custody;
mod adoption;
mod cleanup;
mod session;
mod types;

pub(in crate::range_widget) use adopted_custody::AdoptedPrepublicationCustody;
pub use cleanup::*;
pub use session::{RangePrepublicationCandidate, RangePrepublicationSession};
pub use types::*;

use super::{RangeSurfaceCharge, RangeTextInput, RangeTextInputConfig, RangeTextInputError};

pub(super) fn validate_environment(
    config: &RangeTextInputConfig,
) -> Result<(), RangePrepublicationFailure> {
    let finite_metrics = [
        config.viewport_extent,
        config.overscan,
        config.limits.max_intra_anchor,
        config.limits.max_realized_block_extent,
    ];
    if config.limits.page_bytes < 4
        || config.limits.platform_bytes < 4
        || config.limits.max_surface_bytes == 0
        || config.limits.max_surface_items == 0
        || config.limits.max_realization_work_per_frame == 0
        || config.limits.max_realized_block_extent <= gpui::Pixels::ZERO
        || config.viewport_extent <= gpui::Pixels::ZERO
        || config.overscan < gpui::Pixels::ZERO
        || finite_metrics
            .iter()
            .any(|value| !f32::from(*value).is_finite())
        || config.geometry_limits.max_page_bytes() > config.residency_limits.max_pending_bytes()
        || config.clipboard_limits.max_text_page_bytes()
            > config.residency_limits.max_pending_bytes()
        || config.segmentation_limits.max_page_bytes() > config.residency_limits.max_pending_bytes()
        || config.limits.page_bytes > config.residency_limits.max_pending_bytes()
        || config.limits.platform_bytes > config.residency_limits.max_pending_bytes()
        || config.object_residency_limits.max_pending_requests() < 1
        || config.object_residency_limits.max_resident_objects()
            > config.object_residency_limits.max_pending_objects()
        || config.object_residency_limits.max_resident_bytes()
            > config.object_residency_limits.max_pending_bytes()
    {
        return Err(RangePrepublicationFailure::InvalidEnvironment);
    }
    let charge = initial_widget_owner_charge(config)?;
    if charge.bytes > config.limits.max_surface_bytes
        || charge.items > config.limits.max_surface_items
    {
        return Err(RangePrepublicationFailure::InitialCapacityDenied);
    }
    Ok(())
}

pub(super) fn validate_seed(
    seed: crate::RangeRestorationSeed,
    config: &RangeTextInputConfig,
) -> Result<(), RangePrepublicationFailure> {
    if seed.binding != config.binding
        || seed
            .history
            .is_some_and(|frontier| frontier.binding() != seed.binding)
        || seed.caret != seed.selection.head
        || seed.selection.range().is_err()
        || seed.scroll.intra_anchor < gpui::Pixels::ZERO
        || seed.scroll.intra_anchor > config.limits.max_intra_anchor
        || !f32::from(seed.scroll.intra_anchor).is_finite()
    {
        return Err(RangePrepublicationFailure::SourceMismatch);
    }
    let extent = seed.binding.extent().byte_len();
    if [
        seed.caret,
        seed.selection.anchor,
        seed.selection.head,
        seed.scroll.position,
    ]
    .iter()
    .any(|position| position.byte_offset.get() > extent)
    {
        return Err(RangePrepublicationFailure::SourceMismatch);
    }
    Ok(())
}

pub(super) fn initial_widget_owner_charge(
    config: &RangeTextInputConfig,
) -> Result<RangeSurfaceCharge, RangePrepublicationFailure> {
    let response_custody_capacity = config
        .residency_limits
        .max_pending_requests()
        .checked_add(config.object_residency_limits.max_pending_requests())
        .ok_or(RangePrepublicationFailure::Arithmetic)?;
    let request_capacity =
        super::checked_request_capacity(config).ok_or(RangePrepublicationFailure::Arithmetic)?;
    let request_storage = RangeSurfaceCharge {
        bytes: request_capacity
            .checked_mul(std::mem::size_of::<super::RangeTextInputRequest>())
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
        items: request_capacity,
    };
    let response_custody_storage = RangeSurfaceCharge {
        bytes: response_custody_capacity
            .checked_mul(std::mem::size_of::<
                super::response_custody::RangeResponseCustody,
            >())
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
        items: response_custody_capacity,
    };
    let dispatch = [
        super::realization::DispatchedKeys::<crate::PageRequestKey>::checked_allocation_charge(
            config.residency_limits.max_pending_requests(),
        ),
        super::realization::DispatchedKeys::<crate::ObjectRequestKey>::checked_allocation_charge(
            config.object_residency_limits.max_pending_requests(),
        ),
        super::realization::DispatchedKeys::<crate::MutationKey>::checked_allocation_charge(2),
    ]
    .into_iter()
    .try_fold(RangeSurfaceCharge::default(), |total, charge| {
        let charge = charge?;
        Some(RangeSurfaceCharge {
            bytes: total.bytes.checked_add(charge.bytes)?,
            items: total.items.checked_add(charge.items)?,
        })
    })
    .ok_or(RangePrepublicationFailure::Arithmetic)?;
    let geometry =
        crate::ExactGeometryOwner::initial_required_charge(&config.layout, &config.style)
            .map_err(|_| RangePrepublicationFailure::InvalidEnvironment)?;
    [
        RangeTextInput::realization_owner_charge(),
        request_storage,
        response_custody_storage,
        dispatch,
        crate::RangeResidency::checked_initial_owner_storage_charge(config.residency_limits)
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
        crate::ObjectResidency::checked_initial_owner_storage_charge(
            config.object_residency_limits,
        )
        .ok_or(RangePrepublicationFailure::Arithmetic)?,
        RangeSurfaceCharge {
            bytes: geometry.0,
            items: geometry.1,
        },
    ]
    .into_iter()
    .try_fold(RangeSurfaceCharge::default(), |total, charge| {
        Some(RangeSurfaceCharge {
            bytes: total.bytes.checked_add(charge.bytes)?,
            items: total.items.checked_add(charge.items)?,
        })
    })
    .ok_or(RangePrepublicationFailure::Arithmetic)
}

pub(super) fn adoption_widget_support_charge(
    config: &RangeTextInputConfig,
) -> Result<RangeSurfaceCharge, RangePrepublicationFailure> {
    let initial = initial_widget_owner_charge(config)?;
    let geometry =
        crate::ExactGeometryOwner::initial_required_charge(&config.layout, &config.style)
            .map_err(|_| RangePrepublicationFailure::InvalidEnvironment)?;
    Ok(RangeSurfaceCharge {
        bytes: initial
            .bytes
            .checked_sub(geometry.0)
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
        items: initial
            .items
            .checked_sub(geometry.1)
            .ok_or(RangePrepublicationFailure::Arithmetic)?,
    })
}

impl From<RangeTextInputError> for RangePrepublicationFailure {
    fn from(_: RangeTextInputError) -> Self {
        Self::InvalidEnvironment
    }
}
