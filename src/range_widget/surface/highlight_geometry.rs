use super::*;

pub(in crate::range_widget) type Buffers = (Vec<Bounds<Pixels>>, Vec<Bounds<Pixels>>);

#[cfg(feature = "test-support")]
pub(in crate::range_widget) fn test_object(
    leading: SourcePosition,
    trailing: SourcePosition,
    bounds: Bounds<Pixels>,
) -> RealizedInlineObjectGeometry {
    RealizedInlineObjectGeometry {
        id: InlineObjectId::new(1),
        order: InlineObjectOrder::new(1),
        leading,
        trailing,
        bounds,
        hit_bounds: bounds,
        leading_caret_bounds: bounds,
        trailing_caret_bounds: bounds,
        presentation_index: 0,
    }
}

fn text_bounds(
    maps: &[StreamingLayoutMap],
    selected: Option<ByteRange>,
    line_height: Pixels,
    wrap_width: Pixels,
) -> impl Iterator<Item = Bounds<Pixels>> + Clone + '_ {
    maps.iter()
        .copied()
        .filter(|map| map.logical_position.gap == StreamingObjectGap::no_objects())
        .scan(None, |previous, right| {
            Some((previous.replace(right), right))
        })
        .filter_map(move |(left, right)| {
            let left: StreamingLayoutMap = left?;
            let selected = selected.filter(|range| !range.is_empty())?;
            (right.logical_position.byte_offset > selected.start().get()
                && left.logical_position.byte_offset < selected.end().get())
            .then_some((left, right))
        })
        .flat_map(move |(left, right)| {
            if left.position.y == right.position.y {
                [
                    Some(Bounds::new(
                        left.position,
                        gpui::size(
                            (right.position.x - left.position.x).max(px(1.)),
                            line_height,
                        ),
                    )),
                    None,
                ]
            } else {
                [
                    Some(Bounds::new(
                        left.position,
                        gpui::size((wrap_width - left.position.x).max(px(1.)), line_height),
                    )),
                    Some(Bounds::new(
                        gpui::point(px(0.), right.position.y),
                        gpui::size(right.position.x.max(px(1.)), line_height),
                    )),
                ]
            }
        })
        .flatten()
}

pub(in crate::range_widget) fn prepare(
    maps: &[StreamingLayoutMap],
    objects: &[RealizedInlineObjectGeometry],
    selection: RangeSourceSelection,
    composition: Option<ByteRange>,
    line_height: Pixels,
    wrap_width: Pixels,
    mut admit: impl FnMut(RangeSurfaceCharge) -> bool,
) -> Result<Buffers, crate::RangeTextInputError> {
    let selected = selection.range().ok();
    let byte_range = selected
        .and_then(|range| ByteRange::new(range.start().byte_offset, range.end().byte_offset).ok());
    let selection_bounds = text_bounds(maps, byte_range, line_height, wrap_width).chain(
        objects
            .iter()
            .filter(move |object| {
                byte_range.is_some()
                    && selected.is_some_and(|range| {
                        range
                            .start()
                            .compare_in_revision(object.leading)
                            .is_some_and(|ordering| !ordering.is_gt())
                            && object
                                .trailing
                                .compare_in_revision(range.end())
                                .is_some_and(|ordering| !ordering.is_gt())
                    })
            })
            .map(|object| object.bounds),
    );
    let composition_bounds = text_bounds(maps, composition, line_height, wrap_width);
    let count = |total: usize, _: Bounds<Pixels>| {
        total
            .checked_add(1)
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)
    };
    let selection_count = selection_bounds.clone().try_fold(0, count)?;
    let composition_count = composition_bounds.clone().try_fold(0, count)?;
    let mut check = |selection: usize, composition: usize| {
        let items = selection
            .checked_add(composition)
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let bytes = items
            .checked_mul(std::mem::size_of::<Bounds<Pixels>>())
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        admit(RangeSurfaceCharge { bytes, items })
            .then_some(())
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)
    };
    check(selection_count, composition_count)?;
    let mut selection = Vec::new();
    selection
        .try_reserve_exact(selection_count)
        .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?;
    check(selection.capacity(), composition_count)?;
    let mut composition = Vec::new();
    composition
        .try_reserve_exact(composition_count)
        .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?;
    check(selection.capacity(), composition.capacity())?;
    selection.extend(selection_bounds);
    composition.extend(composition_bounds);
    Ok((selection, composition))
}
