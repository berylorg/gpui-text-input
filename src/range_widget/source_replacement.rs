#[path = "source_replacement/object_removals.rs"]
mod object_removals;
use object_removals::{RemovalTail, prepare_removals};

use super::*;
use crate::{
    MutationCursor, MutationFinishInput, MutationIdentity, MutationKey, MutationLane, MutationPage,
    MutationPageItem, MutationPageKey, MutationPass, MutationPositions, MutationPositionsIntent,
    MutationStreamFinish, MutationTotals, ObjectChange, ObjectCursor, ObjectDemandEnvelope,
    ObjectDirection, ObjectPage, ObjectPageEdgeFact, ObjectPurpose, ObjectRequest, ObjectRequestId,
    ObjectRequestKey, ObjectTarget, SourceRange,
};

#[derive(Clone, Copy, Debug)]
pub(super) enum ProducerMode {
    Dormant,
    Evidence(MutationPass),
    Staging(Option<MutationPass>),
}

#[derive(Debug)]
pub(super) struct LocalReplacementProducer {
    pub(super) range: SourceRange,
    pub(super) text: String,
    pub(super) mode: ProducerMode,
    cursor: Option<ObjectCursor>,
    tail: Option<RemovalTail>,
    pub(super) pending: Option<ObjectRequestKey>,
    complete: bool,
    text_emitted: bool,
    finish_emitted: bool,
    next_cursor: MutationCursor,
    next_ordinal: u64,
    cumulative: MutationIdentity,
    totals: MutationTotals,
}

pub(super) struct PreparedLocalReplacementObjects {
    removals: object_removals::PreparedObjectRemovals,
    page: Option<MutationPage>,
}

impl LocalReplacementProducer {
    pub(super) fn is_complete(&self) -> bool {
        self.complete && self.text_emitted
    }
    pub(super) fn new(range: SourceRange, text: String) -> Self {
        Self {
            range,
            text,
            mode: ProducerMode::Dormant,
            cursor: None,
            tail: None,
            pending: None,
            complete: false,
            text_emitted: false,
            finish_emitted: false,
            next_cursor: MutationCursor::new(0),
            next_ordinal: 0,
            cumulative: MutationIdentity::ROOT,
            totals: MutationTotals::default(),
        }
    }

    pub(super) fn restart(&mut self, mode: ProducerMode) {
        self.mode = mode;
        self.cursor = None;
        self.tail = None;
        self.pending = None;
        self.complete = false;
        self.text_emitted = false;
        self.finish_emitted = false;
        self.next_cursor = MutationCursor::new(0);
        self.next_ordinal = 0;
        self.cumulative = MutationIdentity::ROOT;
        self.totals = MutationTotals::default();
    }

    fn page(
        &mut self,
        key: MutationKey,
        items: Vec<MutationPageItem>,
    ) -> Result<MutationPage, RangeTextInputError> {
        let page = self.preview_page(key, items)?;
        self.accept_page_frontier(&page);
        Ok(page)
    }

    fn preview_page(
        &self,
        key: MutationKey,
        items: Vec<MutationPageItem>,
    ) -> Result<MutationPage, RangeTextInputError> {
        let next = self
            .next_cursor
            .get()
            .checked_add(1)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        self.next_ordinal
            .checked_add(1)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let page = MutationPage::new(
            MutationPageKey::new(
                key,
                MutationLane::Proposal,
                self.next_cursor,
                self.next_ordinal,
                self.cumulative,
            ),
            MutationCursor::new(next),
            items,
        )?;
        self.totals
            .checked_add(page.totals())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        Ok(page)
    }

    fn accept_page_frontier(&mut self, page: &MutationPage) {
        self.next_cursor = page.next_cursor();
        self.next_ordinal = self
            .next_ordinal
            .checked_add(1)
            .expect("prepared ordinal fits");
        self.cumulative = page.cumulative_identity();
        self.totals = self
            .totals
            .checked_add(page.totals())
            .expect("prepared totals fit");
    }

    pub(super) fn finish(&self, original: MutationFinishInput) -> MutationFinishInput {
        MutationFinishInput::with_positions_intent(
            original.key(),
            original.source(),
            MutationStreamFinish {
                next_cursor: self.next_cursor,
                next_ordinal: self.next_ordinal,
                cumulative_identity: self.cumulative,
                totals: self.totals,
            },
            original.intended_extent(),
            original.intended(),
        )
    }
}

impl RangeTextInput {
    pub(super) fn local_replacement_response_is_current(&self, key: ObjectRequestKey) -> bool {
        self.pending_local_mutation.as_ref().is_some_and(|local| {
            local
                .producer
                .as_ref()
                .is_some_and(|producer| producer.pending == Some(key))
        })
    }

    pub(super) fn has_local_replacement_work(&self) -> bool {
        self.pending_local_mutation.as_ref().is_some_and(|local| {
            local.producer.as_ref().is_some_and(|producer| {
                matches!(producer.mode, ProducerMode::Staging(_))
                    && producer.pending.is_none()
                    && !producer.finish_emitted
            })
        })
    }

    pub(super) fn begin_paged_source_replacement(
        &mut self,
        range: SourceRange,
        predecessor: MutationPositions,
        proofs: [Option<crate::range_edit::SourcePositionProof>; 2],
        line_breaks: u64,
        text: String,
        kind: crate::MutationKind,
        cx: &mut Context<Self>,
    ) -> Result<MutationKey, RangeTextInputError> {
        if self.pending_local_mutation.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        if self.requests.len() == self.requests.capacity() {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.check_mutation_payload_capacity(RangeSurfaceCharge {
            bytes: text.capacity(),
            items: usize::from(text.capacity() != 0),
        })?;
        let key = MutationKey::new(
            self.config.binding.binding(),
            self.config.binding.revision(),
            self.next_local_operation()?,
        );
        let proposal = self.claim_local_mutation_proposal(crate::MutationProposal::new(
            key,
            kind,
            predecessor,
            range,
            line_breaks,
        ))?;
        let key = proposal.key();
        let inserted_utf8_bytes =
            u64::try_from(text.len()).map_err(|_| RangeTextInputError::SurfaceCapacity)?;
        let intended = MutationPositionsIntent::ReplacementEnd {
            replacement: range,
            inserted_utf8_bytes,
        };
        let extent = super::interaction::local_successor_extent(
            self.edits.binding().extent(),
            proposal,
            MutationTotals {
                inserted_bytes: text.len() as u64,
                inserted_line_breaks: text.bytes().filter(|byte| *byte == b'\n').count() as u64,
                ..MutationTotals::default()
            },
        )?;
        let empty = MutationStreamFinish {
            next_cursor: MutationCursor::new(0),
            next_ordinal: 0,
            cumulative_identity: MutationIdentity::ROOT,
            totals: MutationTotals::default(),
        };
        let finish =
            MutationFinishInput::with_positions_intent(key, empty, empty, extent, intended);
        let begin = crate::MutationBeginRequest::new(
            proposal,
            MutationCursor::new(0),
            MutationCursor::new(0),
        )
        .with_replayable_producer(crate::MutationProducerIdentity::new(key.operation().get()));
        self.edits.begin(begin)?;
        self.pending_local_mutation = Some(super::interaction::PendingLocalMutation {
            key,
            page: None,
            finish,
            producer: Some(LocalReplacementProducer::new(range, text)),
            evidence_page_acknowledged: false,
        });
        self.admitted_edit_proofs = proofs.into_iter().flatten().collect();
        self.push_request(RangeTextInputRequest::MutationBegin(begin), cx)?;
        self.observe_realization_ownership();
        Ok(key)
    }

    pub(super) fn prepare_local_replacement(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let Some(local) = self.pending_local_mutation.as_ref() else {
            return Ok(());
        };
        let Some(producer) = local.producer.as_ref() else {
            return Ok(());
        };
        if local.page.is_some()
            || producer.pending.is_some()
            || producer.finish_emitted
            || matches!(producer.mode, ProducerMode::Dormant)
        {
            return Ok(());
        }
        if !producer.complete {
            let range = crate::ByteRange::new(
                producer.range.start().byte_offset,
                producer.range.end().byte_offset,
            )?;
            let cursor = producer.cursor;
            let max_objects = self
                .config
                .mutation_limits
                .max_page_objects()
                .min(self.config.mutation_limits.max_page_items())
                .min(self.config.limits.max_realization_work_per_frame);
            let max_bytes = self.config.object_residency_limits.max_pending_bytes();
            let demand = ObjectDemandEnvelope::range(
                range,
                cursor,
                ObjectDirection::Forward,
                max_objects,
                max_bytes,
            )
            .map_err(|_| RangeTextInputError::InvalidLimits)?;
            let key = ObjectRequestKey::new(
                ObjectRequestId::new(self.next_id()),
                self.config.binding.binding(),
                self.config.binding.revision(),
                self.config.presentation_generation,
                ObjectPurpose::MutationPredecessor,
                demand,
            )
            .map_err(|_| RangeTextInputError::InvalidLimits)?;
            self.push_request(
                RangeTextInputRequest::ObjectPage(ObjectRequest::new(key)),
                cx,
            )?;
            self.pending_local_mutation
                .as_mut()
                .unwrap()
                .producer
                .as_mut()
                .unwrap()
                .pending = Some(key);
            return Ok(());
        }
        if !producer.text_emitted {
            let text_bytes = producer.text.len();
            let charge = RangeSurfaceCharge {
                bytes: std::mem::size_of::<MutationPageItem>()
                    .checked_mul(2)
                    .and_then(|bytes| bytes.checked_add(text_bytes))
                    .ok_or(RangeTextInputError::SurfaceCapacity)?,
                items: 3,
            };
            self.check_mutation_payload_capacity(charge)?;
            let local = self.pending_local_mutation.as_mut().unwrap();
            let producer = local.producer.as_mut().unwrap();
            let page = producer.page(
                local.key,
                vec![MutationPageItem::Utf8 {
                    inserted_offset: 0,
                    text: producer.text.clone().into_boxed_str(),
                }],
            )?;
            producer.text_emitted = true;
            local.page = Some(page);
        }
        Ok(())
    }

    pub(super) fn drive_local_replacement(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        self.prepare_local_replacement(cx)?;
        let Some(local) = self.pending_local_mutation.as_ref() else {
            return Ok(());
        };
        let Some(producer) = local.producer.as_ref() else {
            return Ok(());
        };
        let ProducerMode::Staging(pass) = producer.mode else {
            return Ok(());
        };
        if local.page.is_some() {
            if self.requests.len() == self.requests.capacity()
                || !self.mutation_queue_has_capacity(local.key)
            {
                return Err(RangeTextInputError::Busy);
            }
            self.check_mutation_payload_capacity(RangeSurfaceCharge::default())?;
            let key = local.key;
            let page = self
                .pending_local_mutation
                .as_ref()
                .unwrap()
                .page
                .as_ref()
                .unwrap()
                .clone();
            let mut request = crate::MutationPageRequest::new(page.clone());
            let accepted = if let Some(pass) = pass {
                request = request.with_pass(pass);
                self.edits.accept_pass_page(pass, page)
            } else {
                self.edits.accept_page(page)
            };
            if let Err(error) = accepted {
                if self.edits.is_retired(key) {
                    self.finish_local_mutation(key, crate::MutationOutcome::Error, cx);
                }
                return Err(error.into());
            }
            self.pending_local_mutation.as_mut().unwrap().page = None;
            self.commit_prepared_request(RangeTextInputRequest::MutationProposalPage(request));
            cx.notify();
            self.prepare_local_replacement(cx)?;
        }
        let local = self.pending_local_mutation.as_ref().unwrap();
        let producer = local.producer.as_ref().unwrap();
        if producer.complete
            && producer.text_emitted
            && local.page.is_none()
            && !producer.finish_emitted
        {
            if self.requests.len() == self.requests.capacity()
                || !self.mutation_queue_has_capacity(local.key)
            {
                return Err(RangeTextInputError::Busy);
            }
            let finish = producer.finish(local.finish);
            if let Some(pass) = pass {
                self.submit_mutation_pass_finish(pass, finish, cx)?;
            } else {
                self.submit_mutation_finish(finish, cx)?;
            }
            self.pending_local_mutation
                .as_mut()
                .unwrap()
                .producer
                .as_mut()
                .unwrap()
                .finish_emitted = true;
        }
        Ok(())
    }

    pub(super) fn prepare_local_replacement_object_page(
        &self,
        page: &ObjectPage,
    ) -> Result<PreparedLocalReplacementObjects, RangeTextInputError> {
        if !self.dispatched_object_pages.contains(&page.key()) {
            return Err(RangeTextInputError::Stale);
        }
        if self.requests.len() == self.requests.capacity() {
            return Err(RangeTextInputError::Busy);
        }
        let local = self
            .pending_local_mutation
            .as_ref()
            .ok_or(RangeTextInputError::Stale)?;
        let producer = local
            .producer
            .as_ref()
            .filter(|producer| producer.pending == Some(page.key()))
            .ok_or(RangeTextInputError::Stale)?;
        if local.page.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        let limit = self
            .config
            .mutation_limits
            .max_page_objects()
            .min(self.config.mutation_limits.max_page_items())
            .min(self.config.limits.max_realization_work_per_frame);
        self.check_mutation_payload_capacity(RangeSurfaceCharge {
            bytes: limit
                .checked_mul(std::mem::size_of::<MutationPageItem>())
                .and_then(|bytes| bytes.checked_mul(2))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: limit
                .checked_mul(2)
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        })?;
        let mut removals = prepare_removals(producer, page, limit)?;
        let next_page = if removals.items.is_empty() {
            None
        } else {
            Some(producer.preview_page(local.key, std::mem::take(&mut removals.items))?)
        };
        Ok(PreparedLocalReplacementObjects {
            removals,
            page: next_page,
        })
    }

    pub(super) fn commit_local_replacement_object_page(
        &mut self,
        page: ObjectPage,
        prepared: PreparedLocalReplacementObjects,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let local = self
            .pending_local_mutation
            .as_mut()
            .expect("prepared local mutation remains current");
        let producer = local
            .producer
            .as_mut()
            .expect("prepared producer remains current");
        if let Some(page) = prepared.page.as_ref() {
            producer.accept_page_frontier(page);
        }
        producer.cursor = prepared.removals.cursor;
        producer.tail = prepared.removals.tail;
        producer.complete = prepared.removals.complete;
        producer.pending = None;
        local.page = prepared.page;
        drop(prepared.removals.items);
        let released_key = page.key();
        self.dispatched_object_pages.remove(&released_key);
        drop(page);
        self.active_response_processing = Default::default();
        self.commit_prepared_request(RangeTextInputRequest::ReleaseObjectPage(released_key));
        self.drive_local_replacement(cx)?;
        cx.notify();
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/source_replacement.rs"]
mod tests;
