use super::RangeSurfaceCharge;

pub(in crate::range_widget) fn add(
    left: RangeSurfaceCharge,
    right: RangeSurfaceCharge,
) -> Result<RangeSurfaceCharge, crate::RangeTextInputError> {
    Ok(RangeSurfaceCharge {
        bytes: left
            .bytes
            .checked_add(right.bytes)
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
        items: left
            .items
            .checked_add(right.items)
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
    })
}

pub(in crate::range_widget) fn prepare<T>(
    values: Vec<T>,
    retained: &mut RangeSurfaceCharge,
    mut admit: impl FnMut(RangeSurfaceCharge) -> bool,
) -> Result<Box<[T]>, crate::RangeTextInputError> {
    let charge = |items: usize| -> Result<RangeSurfaceCharge, crate::RangeTextInputError> {
        Ok(RangeSurfaceCharge {
            bytes: items
                .checked_mul(std::mem::size_of::<T>())
                .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
            items,
        })
    };
    let allocated = charge(values.capacity())?;
    let final_charge = charge(values.len())?;
    let remainder = RangeSurfaceCharge {
        bytes: retained
            .bytes
            .checked_sub(allocated.bytes)
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
        items: retained
            .items
            .checked_sub(allocated.items)
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
    };
    let next = add(remainder, final_charge)?;
    let peak = if values.capacity() == values.len() {
        *retained
    } else {
        add(*retained, final_charge)?
    };
    if !admit(peak) {
        return Err(crate::RangeTextInputError::SurfaceCapacity);
    }
    let boxed = values.into_boxed_slice();
    *retained = next;
    Ok(boxed)
}
