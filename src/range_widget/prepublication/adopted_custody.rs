use super::RangePrepublicationCleanupLedger;
use crate::RangeSurfaceCharge;

use super::session::{ObjectCustody, TextCustody};

pub(in crate::range_widget) struct AdoptedPrepublicationCustody {
    ledger: RangePrepublicationCleanupLedger,
    text: Vec<TextCustody>,
    objects: Vec<ObjectCustody>,
}

impl AdoptedPrepublicationCustody {
    pub(super) fn new(
        ledger: RangePrepublicationCleanupLedger,
        text: Vec<TextCustody>,
        objects: Vec<ObjectCustody>,
    ) -> Self {
        Self {
            ledger,
            text,
            objects,
        }
    }

    pub(in crate::range_widget) fn charge(&self) -> RangeSurfaceCharge {
        let storage_bytes = self
            .text
            .capacity()
            .checked_mul(std::mem::size_of::<TextCustody>())
            .and_then(|bytes| {
                self.objects
                    .capacity()
                    .checked_mul(std::mem::size_of::<ObjectCustody>())
                    .and_then(|objects| bytes.checked_add(objects))
            })
            .expect("admitted prepublication custody storage fits usize");
        let records = self
            .text
            .len()
            .checked_add(self.objects.len())
            .expect("admitted prepublication custody count fits usize");
        let record = RangePrepublicationCleanupLedger::record_charge();
        RangeSurfaceCharge {
            bytes: storage_bytes
                .checked_add(
                    record
                        .bytes
                        .checked_mul(records)
                        .expect("admitted cleanup record bytes fit usize"),
                )
                .expect("admitted cleanup custody bytes fit usize"),
            items: self
                .text
                .capacity()
                .checked_add(self.objects.capacity())
                .and_then(|items| {
                    record
                        .items
                        .checked_mul(records)
                        .and_then(|records| items.checked_add(records))
                })
                .expect("admitted cleanup custody items fit usize"),
        }
    }

    pub(in crate::range_widget) fn release(&mut self) {
        for custody in self.text.drain(..) {
            self.ledger.mark_token_ready(custody.cleanup);
        }
        for custody in self.objects.drain(..) {
            self.ledger.mark_token_ready(custody.cleanup);
        }
    }
}

impl Drop for AdoptedPrepublicationCustody {
    fn drop(&mut self) {
        self.release();
    }
}
