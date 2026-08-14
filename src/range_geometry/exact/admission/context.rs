use crate::{ByteOffset, PageRequestKey, RangePage};

use super::*;

pub(super) fn admit(
    owner: &mut ExactGeometryOwner,
    mut active: Box<ActiveJob>,
    page: &RangePage,
    expected: PageRequestKey,
    replay: ByteOffset,
    mut budget: AdmissionBudget,
) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
    let origin = active.scanner.cursor_origin.get();
    let feed_start = page.range().start().get().max(origin);
    let malformed = feed_start >= page.range().end().get()
        || page.atoms().iter().any(|atom| {
            atom.fragment_range().end().get() > feed_start
                && atom.fragment_range().start().get() < page.range().end().get()
        });
    if malformed {
        return Err(owner.terminal_failure(
            ExactGeometryError::SourceContract,
            ExactGeometryFailureStage::Scan,
            active,
            &budget,
        ));
    }
    let local_start = match usize::try_from(feed_start - page.range().start().get()) {
        Ok(start) => start,
        Err(_) => {
            return Err(owner.terminal_failure(
                ExactGeometryError::SourceContract,
                ExactGeometryFailureStage::Scan,
                active,
                &budget,
            ));
        }
    };
    let chunk_start = match usize::try_from(feed_start - origin) {
        Ok(start) => start,
        Err(_) => {
            return Err(owner.terminal_failure(
                ExactGeometryError::SourceContract,
                ExactGeometryFailureStage::Scan,
                active,
                &budget,
            ));
        }
    };
    active
        .scanner
        .cursor
        .provide_context(&page.text()[local_start..], chunk_start);
    active.page_use = ActivePageUse::Traverse { anchor: replay };
    finish(owner, active, expected, &mut budget)
}

pub(super) fn defer(
    owner: &mut ExactGeometryOwner,
    mut active: Box<ActiveJob>,
    expected: PageRequestKey,
    required_end: ByteOffset,
    replay: ByteOffset,
    mut budget: AdmissionBudget,
) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
    active.page_use = ActivePageUse::Context {
        required_end,
        replay,
    };
    finish(owner, active, expected, &mut budget)
}

fn finish(
    owner: &mut ExactGeometryOwner,
    mut active: Box<ActiveJob>,
    expected: PageRequestKey,
    budget: &mut AdmissionBudget,
) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
    active.pending = None;
    if let Err(error) = budget.observe(&active, 0, 0) {
        return Err(owner.terminal_failure(
            error,
            ExactGeometryFailureStage::PageCoexistence,
            active,
            budget,
        ));
    }
    owner.high_water_bytes = owner.high_water_bytes.max(budget.peak_bytes);
    owner.high_water_items = owner.high_water_items.max(budget.peak_items);
    owner.active = Some(active);
    Ok(owner.page_admission(
        ExactGeometryProgress::Scanning,
        consumed_page_release(expected),
        budget,
    ))
}
