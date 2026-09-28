use super::*;

pub(super) fn prepare(
    config: &crate::RangeTextInputConfig,
    limit: RangeSurfaceCharge,
) -> Result<(ExactGeometryOwner, RangeResidency, crate::ObjectResidency), RangePrepublicationFailure>
{
    let (bytes, items) =
        ExactGeometryOwner::initial_cloned_required_charge(&config.layout, &config.style)
            .map_err(classify_geometry_error)?;
    let mut geometry_charge = RangeSurfaceCharge { bytes, items };
    let mut text_charge =
        RangeResidency::checked_initial_owner_storage_charge(config.residency_limits)
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
    let mut object_charge = crate::ObjectResidency::checked_initial_owner_storage_charge(
        config.object_residency_limits,
    )
    .ok_or(RangePrepublicationFailure::Arithmetic)?;
    let custody = custody::storage_charge(
        config.residency_limits.max_resident_pages(),
        config.object_residency_limits.max_resident_pages(),
    )
    .ok_or(RangePrepublicationFailure::Arithmetic)?;
    let admit = |geometry, text, objects| {
        let mut charge = add_charge(
            RangeSurfaceCharge {
                bytes: std::mem::size_of::<RangePrepublicationSession>(),
                items: 1,
            },
            custody,
        )?;
        for (owner, inline) in [
            (geometry, std::mem::size_of::<ExactGeometryOwner>()),
            (text, std::mem::size_of::<RangeResidency>()),
            (objects, std::mem::size_of::<crate::ObjectResidency>()),
        ] {
            charge = add_charge(
                charge,
                nested_owner_charge(owner, inline).ok_or(RangePrepublicationFailure::Arithmetic)?,
            )?;
        }
        if !charge_fits(charge, limit) {
            return Err(RangePrepublicationFailure::InitialCapacityDenied);
        }
        Ok(())
    };
    admit(geometry_charge, text_charge, object_charge)?;
    let geometry = ExactGeometryOwner::new(
        config.binding,
        config.presentation_generation,
        config.layout.clone(),
        config.style.clone(),
        config.geometry_limits,
    )
    .map_err(classify_geometry_error)?;
    let counts = geometry.counts();
    geometry_charge = RangeSurfaceCharge {
        bytes: counts.total_bytes(),
        items: counts.total_items(),
    };
    admit(geometry_charge, text_charge, object_charge)?;
    let text = RangeResidency::new(config.binding, config.residency_limits);
    text_charge = text.owner_storage_charge();
    admit(geometry_charge, text_charge, object_charge)?;
    let objects = crate::ObjectResidency::new(
        config.binding,
        config.presentation_generation,
        config.object_residency_limits,
    );
    object_charge = objects.owner_storage_charge();
    admit(geometry_charge, text_charge, object_charge)?;
    Ok((geometry, text, objects))
}
