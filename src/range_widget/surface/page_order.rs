use super::{ByteOffset, RangeSurfaceCharge, SurfacePageIndex};

pub(in crate::range_widget) enum Failure {
    Arithmetic,
    Allocation,
    Refused,
}

pub(in crate::range_widget) fn prepare(
    starts: impl ExactSizeIterator<Item = ByteOffset>,
    mut admit: impl FnMut(RangeSurfaceCharge) -> bool,
) -> Result<Box<[SurfacePageIndex]>, Failure> {
    let charge = |items: usize| {
        Ok(RangeSurfaceCharge {
            bytes: items
                .checked_mul(std::mem::size_of::<SurfacePageIndex>())
                .ok_or(Failure::Arithmetic)?,
            items,
        })
    };
    let count = starts.len();
    if count > 0 && u32::try_from(count - 1).is_err() {
        return Err(Failure::Arithmetic);
    }
    if !admit(charge(count)?) {
        return Err(Failure::Refused);
    }
    let mut order = Vec::new();
    order
        .try_reserve_exact(count)
        .map_err(|_| Failure::Allocation)?;
    if !admit(charge(order.capacity())?) {
        return Err(Failure::Refused);
    }
    for (index, start) in starts.enumerate() {
        order.push(SurfacePageIndex {
            index: index as u32,
            start,
        });
    }
    // The original index preserves stable equal-position order without sort scratch.
    order.sort_unstable_by_key(|entry| (entry.start, entry.index));
    if order.capacity() != order.len() {
        let coexist = order
            .capacity()
            .checked_add(order.len())
            .ok_or(Failure::Arithmetic)?;
        if !admit(charge(coexist)?) {
            return Err(Failure::Refused);
        }
    }
    Ok(order.into_boxed_slice())
}
