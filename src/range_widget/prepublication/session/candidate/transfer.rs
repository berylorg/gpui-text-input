use super::*;

pub(super) fn charge(
    text: usize,
    objects: usize,
) -> Result<RangeSurfaceCharge, RangePrepublicationFailure> {
    add_charge(
        multiply_charge(
            RangeSurfaceCharge {
                bytes: std::mem::size_of::<RangePage>(),
                items: 1,
            },
            text,
        )
        .ok_or(RangePrepublicationFailure::Arithmetic)?,
        multiply_charge(
            RangeSurfaceCharge {
                bytes: std::mem::size_of::<ObjectPage>(),
                items: 1,
            },
            objects,
        )
        .ok_or(RangePrepublicationFailure::Arithmetic)?,
    )
}

pub(super) fn prepare(
    text: usize,
    objects: usize,
    current: RangeSurfaceCharge,
    configured: RangeSurfaceCharge,
    available: RangeSurfaceCharge,
    peak: &mut RangeSurfaceCharge,
) -> Result<Option<(Vec<RangePage>, Vec<ObjectPage>)>, RangePrepublicationFailure> {
    let mut admit = |growth| {
        let required = add_charge(current, growth)?;
        peak.bytes = peak.bytes.max(required.bytes);
        peak.items = peak.items.max(required.items);
        if !charge_fits(required, configured) {
            return Err(RangePrepublicationFailure::TerminalCapacity);
        }
        Ok(charge_fits(required, available))
    };
    if !admit(charge(text, objects)?)? {
        return Ok(None);
    }
    let mut pages = Vec::new();
    pages
        .try_reserve_exact(text)
        .map_err(|_| RangePrepublicationFailure::TerminalCapacity)?;
    if !admit(charge(pages.capacity(), objects)?)? {
        return Ok(None);
    }
    let mut object_pages = Vec::new();
    object_pages
        .try_reserve_exact(objects)
        .map_err(|_| RangePrepublicationFailure::TerminalCapacity)?;
    if !admit(charge(pages.capacity(), object_pages.capacity())?)? {
        return Ok(None);
    }
    Ok(Some((pages, object_pages)))
}
