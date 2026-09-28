use super::*;

pub(super) fn prepare<T>(
    current: RangeSurfaceCharge,
    configured: RangeSurfaceCharge,
    available: RangeSurfaceCharge,
    peak: &mut RangeSurfaceCharge,
    prepare: impl FnOnce(
        &mut dyn FnMut(RangeSurfaceCharge) -> bool,
    ) -> Result<T, crate::RangeTextInputError>,
) -> Result<Option<T>, RangePrepublicationFailure> {
    let mut refusal = None;
    let mut blocked = false;
    let result = prepare(&mut |growth| {
        let required = match add_charge(current, growth) {
            Ok(required) => required,
            Err(error) => {
                refusal = Some(error);
                return false;
            }
        };
        peak.bytes = peak.bytes.max(required.bytes);
        peak.items = peak.items.max(required.items);
        if !charge_fits(required, configured) {
            refusal = Some(RangePrepublicationFailure::TerminalCapacity);
            return false;
        }
        blocked = !charge_fits(required, available);
        !blocked
    });
    if let Some(error) = refusal {
        return Err(error);
    }
    if blocked {
        return Ok(None);
    }
    result.map(Some).map_err(classify_widget_error)
}

#[cfg(feature = "test-support")]
impl RangePrepublicationSession {
    pub fn test_prepare_highlight_geometry(
        maps: &[gpui::StreamingLayoutMap],
        objects: &[(
            crate::SourcePosition,
            crate::SourcePosition,
            gpui::Bounds<gpui::Pixels>,
        )],
        selection: crate::RangeSourceSelection,
        composition: Option<crate::ByteRange>,
        current: RangeSurfaceCharge,
        configured: RangeSurfaceCharge,
        available: RangeSurfaceCharge,
        peak: &mut RangeSurfaceCharge,
    ) -> Result<
        Option<crate::range_widget::surface::highlight_geometry::Buffers>,
        RangePrepublicationFailure,
    > {
        let objects: Vec<_> = objects
            .iter()
            .map(|&(leading, trailing, bounds)| {
                crate::range_widget::surface::highlight_geometry::test_object(
                    leading, trailing, bounds,
                )
            })
            .collect();
        prepare(current, configured, available, peak, |admit| {
            crate::range_widget::surface::highlight_geometry::prepare(
                maps,
                &objects,
                selection,
                composition,
                gpui::px(10.),
                gpui::px(100.),
                admit,
            )
        })
    }
}

#[cfg(feature = "test-support")]
impl RangePrepublicationSession {
    pub fn test_box_collections(
        values: [Vec<u64>; 2],
        mut retained: RangeSurfaceCharge,
        current: RangeSurfaceCharge,
        configured: RangeSurfaceCharge,
        available: RangeSurfaceCharge,
        peak: &mut RangeSurfaceCharge,
    ) -> Result<Option<([Box<[u64]>; 2], RangeSurfaceCharge)>, RangePrepublicationFailure> {
        prepare(current, configured, available, peak, |admit| {
            let [first, second] = values;
            let first = crate::range_widget::surface::boxed_collection::prepare(
                first,
                &mut retained,
                &mut *admit,
            )?;
            let second = crate::range_widget::surface::boxed_collection::prepare(
                second,
                &mut retained,
                admit,
            )?;
            Ok(([first, second], retained))
        })
    }
}
