use super::*;
use crate::range_widget::surface::{SurfacePageIndex, page_order};

pub(super) fn prepare(
    starts: impl ExactSizeIterator<Item = crate::ByteOffset>,
    current: RangeSurfaceCharge,
    configured: RangeSurfaceCharge,
    available: RangeSurfaceCharge,
    peak: &mut RangeSurfaceCharge,
) -> Result<Option<Box<[SurfacePageIndex]>>, RangePrepublicationFailure> {
    let mut refusal = None;
    let result = page_order::prepare(starts, |growth| {
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
        Err(page_order::Failure::Refused) => refusal.map_or(Ok(None), Err),
        Err(page_order::Failure::Arithmetic) => Err(RangePrepublicationFailure::Arithmetic),
        Err(page_order::Failure::Allocation) => Err(RangePrepublicationFailure::TerminalCapacity),
    }
}

#[cfg(feature = "test-support")]
impl RangePrepublicationSession {
    pub fn test_prepare_page_order(
        starts: &[crate::ByteOffset],
        current: RangeSurfaceCharge,
        configured: RangeSurfaceCharge,
        available: RangeSurfaceCharge,
        peak: &mut RangeSurfaceCharge,
    ) -> Result<Option<Vec<u32>>, RangePrepublicationFailure> {
        prepare(starts.iter().copied(), current, configured, available, peak)
            .map(|order| order.map(|order| order.iter().map(|entry| entry.index).collect()))
    }
}
