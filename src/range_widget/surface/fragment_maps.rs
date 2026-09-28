use super::{
    RangeSurfaceCharge, StreamingBoundaryKind, StreamingLayoutFragment, StreamingLayoutMap,
};

pub(in crate::range_widget) enum Failure {
    Arithmetic,
    Allocation,
    Refused,
}

pub(in crate::range_widget) fn owned_maps(
    fragments: &[StreamingLayoutFragment],
) -> impl Iterator<Item = StreamingLayoutMap> + Clone + '_ {
    fragments.iter().flat_map(|fragment| {
        let maps = match fragment {
            StreamingLayoutFragment::Text(fragment) => fragment.maps(),
            StreamingLayoutFragment::OversizeAtom(fragment) => fragment.maps(),
            StreamingLayoutFragment::InlineObject(fragment) => fragment.maps(),
            StreamingLayoutFragment::Boundary(fragment) => fragment.maps(),
        };
        let count = match fragment {
            StreamingLayoutFragment::Text(_) => maps.len().saturating_sub(1),
            StreamingLayoutFragment::OversizeAtom(_) => maps.len().min(1),
            StreamingLayoutFragment::InlineObject(_) => maps.len(),
            StreamingLayoutFragment::Boundary(fragment) => match fragment.kind {
                StreamingBoundaryKind::LogicalLine if maps.len() > 1 => 1,
                StreamingBoundaryKind::EndOfSource => maps.len().min(1),
                StreamingBoundaryKind::LogicalLine => 0,
            },
        };
        maps[..count].iter().copied()
    })
}

pub(in crate::range_widget) fn prepare(
    maps: impl Iterator<Item = StreamingLayoutMap> + Clone,
    mut admit: impl FnMut(RangeSurfaceCharge) -> bool,
) -> Result<Vec<StreamingLayoutMap>, Failure> {
    let mut previous = None;
    let maps = maps.filter(move |map| {
        let keep = previous != Some(map.logical_position);
        previous = Some(map.logical_position);
        keep
    });
    let count = maps
        .clone()
        .try_fold(0usize, |count, _| count.checked_add(1))
        .ok_or(Failure::Arithmetic)?;
    let charge = |items: usize| {
        Ok(RangeSurfaceCharge {
            bytes: items
                .checked_mul(std::mem::size_of::<StreamingLayoutMap>())
                .ok_or(Failure::Arithmetic)?,
            items,
        })
    };
    if !admit(charge(count)?) {
        return Err(Failure::Refused);
    }
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| Failure::Allocation)?;
    if !admit(charge(result.capacity())?) {
        return Err(Failure::Refused);
    }
    for map in maps {
        result.push(map);
    }
    Ok(result)
}
