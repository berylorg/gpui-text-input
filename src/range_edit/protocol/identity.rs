use super::{
    MutationCursor, MutationIdentity, MutationLane, MutationPageItem, MutationPageKey,
    MutationStreamFinish, MutationTotals,
};
use crate::{
    AtomChange, ByteRange, InlineObjectGap, LogicalExtent, MutationKind, MutationPositions,
    MutationProposal, ObjectChange, ObjectTarget, SourcePosition, SuccessorObject,
};

pub(crate) fn canonical_begin_identity(
    proposal: MutationProposal,
    base_extent: LogicalExtent,
    initial_source_cursor: MutationCursor,
    initial_proposal_cursor: MutationCursor,
) -> MutationIdentity {
    let mut digest = IdentityBuilder::new(MutationIdentity::ROOT);
    digest.bytes(b"gpui-text-input/mutation-begin/v1");
    digest.u64(proposal.key().binding().get());
    digest.u64(proposal.key().base_revision().get());
    digest.u64(proposal.key().operation().get());
    digest.u64(match proposal.kind() {
        MutationKind::Edit => 0,
        MutationKind::Undo => 1,
        MutationKind::Redo => 2,
    });
    digest.extent(base_extent);
    digest.positions(proposal.predecessor());
    digest.position(proposal.replacement().start());
    digest.position(proposal.replacement().end());
    digest.u64(proposal.replacement_line_breaks());
    digest.u64(initial_source_cursor.get());
    for word in MutationIdentity::ROOT.words() {
        digest.u64(word);
    }
    digest.u64(initial_proposal_cursor.get());
    for word in MutationIdentity::ROOT.words() {
        digest.u64(word);
    }
    digest.finish()
}

pub(crate) fn canonical_finish_identity(
    proposal: MutationProposal,
    base_extent: LogicalExtent,
    initial_source_cursor: MutationCursor,
    initial_proposal_cursor: MutationCursor,
    source: MutationStreamFinish,
    proposal_finish: MutationStreamFinish,
    intended_extent: LogicalExtent,
    intended: MutationPositions,
    combined_totals: MutationTotals,
) -> MutationIdentity {
    let mut digest = IdentityBuilder::new(MutationIdentity::ROOT);
    digest.bytes(b"gpui-text-input/mutation-finish/v1");
    digest.u64(proposal.key().binding().get());
    digest.u64(proposal.key().base_revision().get());
    digest.u64(proposal.key().operation().get());
    digest.u64(match proposal.kind() {
        MutationKind::Edit => 0,
        MutationKind::Undo => 1,
        MutationKind::Redo => 2,
    });
    digest.extent(base_extent);
    digest.positions(proposal.predecessor());
    digest.position(proposal.replacement().start());
    digest.position(proposal.replacement().end());
    digest.u64(proposal.replacement_line_breaks());
    digest.u64(initial_source_cursor.get());
    for word in MutationIdentity::ROOT.words() {
        digest.u64(word);
    }
    digest.u64(initial_proposal_cursor.get());
    for word in MutationIdentity::ROOT.words() {
        digest.u64(word);
    }
    digest.stream(source);
    digest.stream(proposal_finish);
    digest.extent(intended_extent);
    digest.positions(intended);
    digest.totals(combined_totals);
    digest.finish()
}

pub(crate) fn chain_identity(
    prior: MutationIdentity,
    page: MutationIdentity,
    totals: MutationTotals,
) -> MutationIdentity {
    let mut digest = IdentityBuilder::new(prior);
    for word in page.words() {
        digest.u64(word);
    }
    digest.u64(totals.pages);
    digest.u64(totals.items);
    digest.u64(totals.retained_bytes);
    digest.u64(totals.inserted_bytes);
    digest.u64(totals.inserted_line_breaks);
    digest.u64(totals.objects);
    digest.u64(totals.object_bytes);
    digest.u64(totals.presentation_bytes);
    digest.finish()
}

pub(super) fn canonical_page_identity(
    key: MutationPageKey,
    next_cursor: MutationCursor,
    items: &[MutationPageItem],
) -> MutationIdentity {
    let mut digest = IdentityBuilder::new(MutationIdentity::ROOT);
    digest.u64(key.key().binding().get());
    digest.u64(key.key().base_revision().get());
    digest.u64(key.key().operation().get());
    digest.u64(match key.lane() {
        MutationLane::Source => 0,
        MutationLane::Proposal => 1,
    });
    digest.u64(key.cursor().get());
    digest.u64(key.ordinal());
    for word in key.prior().words() {
        digest.u64(word);
    }
    digest.u64(next_cursor.get());
    digest.u64(items.len() as u64);
    for item in items {
        match item {
            MutationPageItem::Utf8 {
                inserted_offset,
                text,
            } => {
                digest.u64(0);
                digest.u64(*inserted_offset);
                digest.bytes(text.as_bytes());
            }
            MutationPageItem::Atom(AtomChange::Insert {
                id,
                inserted_range,
                fallback_copy,
            }) => {
                digest.u64(1);
                digest.u64(id.get());
                digest.range(*inserted_range);
                digest.bytes(fallback_copy.as_bytes());
            }
            MutationPageItem::Atom(AtomChange::Remove { id, source_range }) => {
                digest.u64(2);
                digest.u64(id.get());
                digest.range(*source_range);
            }
            MutationPageItem::Object(change) => {
                digest.u64(3);
                encode_object_change(&mut digest, *change);
            }
        }
    }
    digest.finish()
}

fn encode_object_change(digest: &mut IdentityBuilder, change: ObjectChange) {
    match change {
        ObjectChange::Insert { object } => {
            digest.u64(0);
            digest.object(object);
        }
        ObjectChange::Remove { target } => {
            digest.u64(1);
            digest.target(target);
        }
        ObjectChange::Replace { target, object } => {
            digest.u64(2);
            digest.target(target);
            digest.object(object);
        }
        ObjectChange::Move { target, object } => {
            digest.u64(3);
            digest.target(target);
            digest.object(object);
        }
    }
}

struct IdentityBuilder([u64; 4]);

impl IdentityBuilder {
    fn new(seed: MutationIdentity) -> Self {
        Self(seed.words())
    }

    fn u64(&mut self, value: u64) {
        const PRIMES: [u64; 4] = [
            0x0000_0100_0000_01b3,
            0x9e37_79b1_85eb_ca87,
            0xc2b2_ae3d_27d4_eb4f,
            0x1656_67b1_9e37_79f9,
        ];
        for (index, word) in self.0.iter_mut().enumerate() {
            *word ^= value.rotate_left((index as u32).saturating_mul(13));
            *word = word.wrapping_mul(PRIMES[index]);
            *word ^= *word >> (17 + index as u32);
        }
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.u64(bytes.len() as u64);
        for chunk in bytes.chunks(8) {
            let mut word = [0; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.u64(u64::from_le_bytes(word));
        }
    }

    fn range(&mut self, range: ByteRange) {
        self.u64(range.start().get());
        self.u64(range.end().get());
    }

    fn extent(&mut self, extent: LogicalExtent) {
        self.u64(extent.byte_len());
        self.u64(extent.line_count());
    }

    fn positions(&mut self, positions: MutationPositions) {
        self.position(positions.caret());
        self.position(positions.selection_anchor());
        self.position(positions.selection_head());
    }

    fn totals(&mut self, totals: MutationTotals) {
        self.u64(totals.pages);
        self.u64(totals.items);
        self.u64(totals.retained_bytes);
        self.u64(totals.inserted_bytes);
        self.u64(totals.inserted_line_breaks);
        self.u64(totals.objects);
        self.u64(totals.object_bytes);
        self.u64(totals.presentation_bytes);
    }

    fn stream(&mut self, finish: MutationStreamFinish) {
        self.u64(finish.next_cursor.get());
        self.u64(finish.next_ordinal);
        for word in finish.cumulative_identity.words() {
            self.u64(word);
        }
        self.totals(finish.totals);
    }

    fn position(&mut self, position: SourcePosition) {
        self.u64(position.byte_offset.get());
        match position.gap {
            InlineObjectGap::NoObjects => self.u64(0),
            InlineObjectGap::Before(next) => {
                self.u64(1);
                self.neighbor(next);
            }
            InlineObjectGap::Between {
                preceding,
                following,
            } => {
                self.u64(2);
                self.neighbor(preceding);
                self.neighbor(following);
            }
            InlineObjectGap::After(previous) => {
                self.u64(3);
                self.neighbor(previous);
            }
        }
    }

    fn neighbor(&mut self, neighbor: crate::InlineObjectNeighbor) {
        self.u128(neighbor.id().get());
        self.u128(neighbor.order().get());
    }

    fn target(&mut self, target: ObjectTarget) {
        self.position(target.range().start());
        self.position(target.range().end());
        self.u128(target.id().get());
        self.u128(target.order().get());
    }

    fn object(&mut self, object: SuccessorObject) {
        self.u128(object.id().get());
        self.u64(object.anchor().get());
        self.u128(object.order().get());
        self.u64(object.retained_bytes() as u64);
        self.u64(object.presentation_bytes() as u64);
    }

    fn finish(self) -> MutationIdentity {
        MutationIdentity(self.0)
    }

    fn u128(&mut self, value: u128) {
        self.u64(value as u64);
        self.u64((value >> 64) as u64);
    }
}
