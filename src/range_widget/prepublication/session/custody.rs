use super::*;

#[derive(Clone, Copy)]
pub(in crate::range_widget) struct TextCustody {
    pub(in crate::range_widget) page: PageId,
    pub(in crate::range_widget) cleanup: RangePrepublicationCleanupToken,
}

#[derive(Clone, Copy)]
pub(in crate::range_widget) struct ObjectCustody {
    pub(in crate::range_widget) page: ObjectPageId,
    pub(in crate::range_widget) cleanup: RangePrepublicationCleanupToken,
}

impl RangePrepublicationSession {
    pub(super) fn retain_text_custody(
        &mut self,
        page: PageId,
        cleanup: RangePrepublicationCleanupToken,
    ) -> Result<(), RangePrepublicationFailure> {
        self.release_replaced_text_custody(page);
        self.release_evicted_text_custody();
        if self.text_custody.len() == self.text_custody.capacity() {
            return Err(RangePrepublicationFailure::TerminalCapacity);
        }
        self.text_custody.push(TextCustody { page, cleanup });
        Ok(())
    }

    pub(super) fn retain_object_custody(
        &mut self,
        page: ObjectPageId,
        cleanup: RangePrepublicationCleanupToken,
    ) -> Result<(), RangePrepublicationFailure> {
        self.release_replaced_object_custody(page);
        self.release_evicted_object_custody();
        if self.object_custody.len() == self.object_custody.capacity() {
            return Err(RangePrepublicationFailure::TerminalCapacity);
        }
        self.object_custody.push(ObjectCustody { page, cleanup });
        Ok(())
    }

    pub(super) fn release_all_resident_custody(&mut self) {
        for custody in self.text_custody.drain(..) {
            self.environment.cleanup().mark_token_ready(custody.cleanup);
        }
        for custody in self.object_custody.drain(..) {
            self.environment.cleanup().mark_token_ready(custody.cleanup);
        }
    }

    pub(super) fn custody_storage_charge(&self) -> Option<RangeSurfaceCharge> {
        Some(RangeSurfaceCharge {
            bytes: self
                .text_custody
                .capacity()
                .checked_mul(std::mem::size_of::<TextCustody>())?
                .checked_add(
                    self.object_custody
                        .capacity()
                        .checked_mul(std::mem::size_of::<ObjectCustody>())?,
                )?,
            items: self
                .text_custody
                .capacity()
                .checked_add(self.object_custody.capacity())?,
        })
    }

    fn release_replaced_text_custody(&mut self, page: PageId) {
        let ledger = self.environment.cleanup().clone();
        self.text_custody.retain(|custody| {
            if custody.page == page {
                ledger.mark_token_ready(custody.cleanup);
                false
            } else {
                true
            }
        });
    }

    fn release_replaced_object_custody(&mut self, page: ObjectPageId) {
        let ledger = self.environment.cleanup().clone();
        self.object_custody.retain(|custody| {
            if custody.page == page {
                ledger.mark_token_ready(custody.cleanup);
                false
            } else {
                true
            }
        });
    }

    fn release_evicted_text_custody(&mut self) {
        let residency = &self.residency;
        let ledger = self.environment.cleanup().clone();
        self.text_custody.retain(|custody| {
            if residency.peek_page_by_id(custody.page).is_none() {
                ledger.mark_token_ready(custody.cleanup);
                false
            } else {
                true
            }
        });
    }

    fn release_evicted_object_custody(&mut self) {
        let residency = &self.object_residency;
        let ledger = self.environment.cleanup().clone();
        self.object_custody.retain(|custody| {
            if residency.peek_page_by_id(custody.page).is_none() {
                ledger.mark_token_ready(custody.cleanup);
                false
            } else {
                true
            }
        });
    }
}
