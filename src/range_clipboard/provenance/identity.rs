use super::*;
use crate::InlineObjectGap;

pub(super) fn canonical_page_identity(
    key: ClipboardProvenancePageKey,
    cursor: ClipboardProvenanceCursor,
    prior_identity: ClipboardProvenanceIdentity,
    next: ClipboardProvenanceCursor,
    items: &[ClipboardProvenanceItem],
) -> ClipboardProvenanceIdentity {
    let mut identity = IdentityBuilder::new(ClipboardProvenanceIdentity::ROOT);
    identity.tag(1);
    identity.clipboard_key(key.clipboard);
    identity.u64(key.page_ordinal);
    identity.cursor(cursor);
    identity.identity(prior_identity);
    identity.cursor(next);
    identity.usize(items.len());
    for item in items {
        identity.inline_id(item.object_id);
        identity.u64(item.source_anchor.get());
        identity.inline_order(item.source_order);
        identity.byte_range(item.output_range);
    }
    identity.finish()
}

pub(super) fn canonical_cumulative_identity(
    prior: ClipboardProvenanceIdentity,
    page: ClipboardProvenanceIdentity,
    next: ClipboardProvenanceCursor,
    item_count: u64,
    fallback_bytes: u64,
) -> ClipboardProvenanceIdentity {
    let mut identity = IdentityBuilder::new(prior);
    identity.tag(2);
    identity.identity(page);
    identity.cursor(next);
    identity.u64(item_count);
    identity.u64(fallback_bytes);
    identity.finish()
}

pub(super) fn canonical_final_identity(
    clipboard: ClipboardKey,
    page_count: u64,
    item_count: u64,
    fallback_bytes: u64,
    output_bytes: u64,
    prior: ClipboardProvenanceIdentity,
    text: &[u8],
) -> ClipboardProvenanceIdentity {
    let mut identity = IdentityBuilder::new(prior);
    identity.tag(3);
    identity.clipboard_key(clipboard);
    identity.u64(page_count);
    identity.u64(item_count);
    identity.u64(fallback_bytes);
    identity.u64(output_bytes);
    identity.identity(prior);
    identity.bytes(text);
    identity.finish()
}

struct IdentityBuilder([u64; 4]);

impl IdentityBuilder {
    fn new(seed: ClipboardProvenanceIdentity) -> Self {
        Self(seed.words())
    }

    fn tag(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn usize(&mut self, value: usize) {
        self.u64(u64::try_from(value).unwrap_or(u64::MAX));
    }

    fn u64(&mut self, value: u64) {
        self.absorb(&value.to_le_bytes());
    }

    fn identity(&mut self, value: ClipboardProvenanceIdentity) {
        for word in value.words() {
            self.u64(word);
        }
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.absorb(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
        self.absorb(bytes);
    }

    fn absorb(&mut self, bytes: &[u8]) {
        for (index, byte) in bytes.iter().copied().enumerate() {
            let lane = index & 3;
            self.0[lane] ^= u64::from(byte).wrapping_add((index as u64).rotate_left(17));
            self.0[lane] = self.0[lane]
                .rotate_left(13 + lane as u32 * 7)
                .wrapping_mul(0x9e37_79b1_85eb_ca87 ^ ((lane as u64) << 32));
            self.0[(lane + 1) & 3] ^= self.0[lane].rotate_left(29);
        }
    }

    fn clipboard_key(&mut self, key: ClipboardKey) {
        self.u64(key.id().get());
        self.u64(key.binding().get());
        self.u64(key.revision().get());
        self.source_position(key.selection().start());
        self.source_position(key.selection().end());
        self.source_position(key.predecessor().caret());
        self.source_position(key.predecessor().selection_anchor());
        self.source_position(key.predecessor().selection_head());
    }

    fn source_position(&mut self, position: crate::SourcePosition) {
        self.u64(position.byte_offset.get());
        match position.gap {
            InlineObjectGap::NoObjects => self.tag(0),
            InlineObjectGap::Before(neighbor) => {
                self.tag(1);
                self.neighbor(neighbor);
            }
            InlineObjectGap::Between {
                preceding,
                following,
            } => {
                self.tag(2);
                self.neighbor(preceding);
                self.neighbor(following);
            }
            InlineObjectGap::After(neighbor) => {
                self.tag(3);
                self.neighbor(neighbor);
            }
        }
    }

    fn neighbor(&mut self, neighbor: crate::InlineObjectNeighbor) {
        self.inline_id(neighbor.id());
        self.inline_order(neighbor.order());
    }

    fn cursor(&mut self, cursor: ClipboardProvenanceCursor) {
        match cursor.preceding_object {
            Some(object) => {
                self.tag(1);
                self.inline_id(object.id());
                self.u64(object.anchor().get());
                self.inline_order(object.order());
            }
            None => self.tag(0),
        }
        self.u64(cursor.item_ordinal);
        self.u64(cursor.output_offset.get());
    }

    fn inline_id(&mut self, id: crate::InlineObjectId) {
        self.bytes(&id.get().to_le_bytes());
    }

    fn inline_order(&mut self, order: crate::InlineObjectOrder) {
        self.bytes(&order.get().to_le_bytes());
    }

    fn byte_range(&mut self, range: ByteRange) {
        self.u64(range.start().get());
        self.u64(range.end().get());
    }

    fn finish(mut self) -> ClipboardProvenanceIdentity {
        for round in 0..12 {
            let lane = round & 3;
            self.0[lane] ^= self.0[(lane + 1) & 3].rotate_left(11 + round as u32);
            self.0[lane] = self.0[lane].wrapping_mul(0xd6e8_feb8_6659_fd93);
        }
        ClipboardProvenanceIdentity(self.0)
    }
}
