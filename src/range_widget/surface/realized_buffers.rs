use super::{RangeSurfaceCharge, RealizedInlineObjectGeometry, RealizedObjectGapGeometry};

pub(in crate::range_widget) type Buffers = (
    Vec<RealizedInlineObjectGeometry>,
    Vec<RealizedObjectGapGeometry>,
);

pub(in crate::range_widget) enum Failure {
    Arithmetic,
    Allocation,
    Refused,
}

pub(in crate::range_widget) fn prepare(
    objects: usize,
    gaps: usize,
    mut admit: impl FnMut(RangeSurfaceCharge) -> bool,
) -> Result<Buffers, Failure> {
    let charge = |objects: usize, gaps: usize| {
        Ok(RangeSurfaceCharge {
            bytes: objects
                .checked_mul(std::mem::size_of::<RealizedInlineObjectGeometry>())
                .and_then(|bytes| {
                    bytes.checked_add(
                        gaps.checked_mul(std::mem::size_of::<RealizedObjectGapGeometry>())?,
                    )
                })
                .ok_or(Failure::Arithmetic)?,
            items: objects.checked_add(gaps).ok_or(Failure::Arithmetic)?,
        })
    };
    if !admit(charge(objects, gaps)?) {
        return Err(Failure::Refused);
    }
    let mut object_buffer = Vec::new();
    object_buffer
        .try_reserve_exact(objects)
        .map_err(|_| Failure::Allocation)?;
    if !admit(charge(object_buffer.capacity(), gaps)?) {
        return Err(Failure::Refused);
    }
    let mut gap_buffer = Vec::new();
    gap_buffer
        .try_reserve_exact(gaps)
        .map_err(|_| Failure::Allocation)?;
    if !admit(charge(object_buffer.capacity(), gap_buffer.capacity())?) {
        return Err(Failure::Refused);
    }
    Ok((object_buffer, gap_buffer))
}

pub(in crate::range_widget) fn counts(
    fragments: &[super::StreamingLayoutFragment],
    maps: &[super::StreamingLayoutMap],
) -> (usize, usize) {
    (
        fragments
            .iter()
            .filter(|fragment| matches!(fragment, super::StreamingLayoutFragment::InlineObject(_)))
            .count(),
        maps.iter()
            .filter(|map| map.logical_position.gap != super::StreamingObjectGap::no_objects())
            .count(),
    )
}
