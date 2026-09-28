use super::*;
use crate::range_widget::surface::realized_buffers;

pub(super) fn prepare(
    objects: usize,
    gaps: usize,
    current: RangeSurfaceCharge,
    configured: RangeSurfaceCharge,
    available: RangeSurfaceCharge,
    peak: &mut RangeSurfaceCharge,
) -> Result<Option<realized_buffers::Buffers>, RangePrepublicationFailure> {
    let mut refusal = None;
    let result = realized_buffers::prepare(objects, gaps, |growth| {
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
        Ok(buffers) => Ok(Some(buffers)),
        Err(realized_buffers::Failure::Refused) => refusal.map_or(Ok(None), Err),
        Err(realized_buffers::Failure::Arithmetic) => Err(RangePrepublicationFailure::Arithmetic),
        Err(realized_buffers::Failure::Allocation) => {
            Err(RangePrepublicationFailure::TerminalCapacity)
        }
    }
}

#[cfg(feature = "test-support")]
impl RangePrepublicationSession {
    pub fn test_prepare_realized_buffers(
        objects: usize,
        gaps: usize,
        current: RangeSurfaceCharge,
        configured: RangeSurfaceCharge,
        available: RangeSurfaceCharge,
        peak: &mut RangeSurfaceCharge,
    ) -> Result<Option<realized_buffers::Buffers>, RangePrepublicationFailure> {
        prepare(objects, gaps, current, configured, available, peak)
    }
}
