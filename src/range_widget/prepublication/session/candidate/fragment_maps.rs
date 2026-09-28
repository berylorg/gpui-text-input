use super::*;
use crate::range_widget::surface::fragment_maps;
use gpui::StreamingLayoutMap;

pub(super) fn prepare(
    maps: impl Iterator<Item = StreamingLayoutMap> + Clone,
    current: RangeSurfaceCharge,
    configured: RangeSurfaceCharge,
    available: RangeSurfaceCharge,
    peak: &mut RangeSurfaceCharge,
) -> Result<Option<Vec<StreamingLayoutMap>>, RangePrepublicationFailure> {
    let mut refusal = None;
    let result = fragment_maps::prepare(maps, |growth| {
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
        charge_fits(required, available)
    });
    match result {
        Ok(order) => Ok(Some(order)),
        Err(fragment_maps::Failure::Refused) => refusal.map_or(Ok(None), Err),
        Err(fragment_maps::Failure::Arithmetic) => Err(RangePrepublicationFailure::Arithmetic),
        Err(fragment_maps::Failure::Allocation) => {
            Err(RangePrepublicationFailure::TerminalCapacity)
        }
    }
}

#[cfg(feature = "test-support")]
impl RangePrepublicationSession {
    pub fn test_prepare_fragment_maps(
        maps: &[StreamingLayoutMap],
        current: RangeSurfaceCharge,
        configured: RangeSurfaceCharge,
        available: RangeSurfaceCharge,
        peak: &mut RangeSurfaceCharge,
    ) -> Result<Option<Vec<StreamingLayoutMap>>, RangePrepublicationFailure> {
        prepare(maps.iter().copied(), current, configured, available, peak)
    }
}
