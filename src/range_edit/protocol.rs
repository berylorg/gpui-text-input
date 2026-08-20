use super::*;

mod identity;

use identity::canonical_page_identity;
pub(crate) use identity::chain_identity;
pub(crate) use identity::{canonical_begin_identity, canonical_finish_identity};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MutationCursor(u64);

impl MutationCursor {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MutationLane {
    Source,
    Proposal,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MutationIdentity([u64; 4]);

impl MutationIdentity {
    pub const ROOT: Self = Self([
        0x6a09_e667_f3bc_c908,
        0xbb67_ae85_84ca_a73b,
        0x3c6e_f372_fe94_f82b,
        0xa54f_f53a_5f1d_36f1,
    ]);

    pub const fn new(words: [u64; 4]) -> Self {
        Self(words)
    }

    pub const fn words(self) -> [u64; 4] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MutationTotals {
    pub pages: u64,
    pub items: u64,
    pub retained_bytes: u64,
    pub inserted_bytes: u64,
    pub inserted_line_breaks: u64,
    pub objects: u64,
    pub object_bytes: u64,
    pub presentation_bytes: u64,
}

impl MutationTotals {
    pub(crate) fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            pages: self.pages.checked_add(other.pages)?,
            items: self.items.checked_add(other.items)?,
            retained_bytes: self.retained_bytes.checked_add(other.retained_bytes)?,
            inserted_bytes: self.inserted_bytes.checked_add(other.inserted_bytes)?,
            inserted_line_breaks: self
                .inserted_line_breaks
                .checked_add(other.inserted_line_breaks)?,
            objects: self.objects.checked_add(other.objects)?,
            object_bytes: self.object_bytes.checked_add(other.object_bytes)?,
            presentation_bytes: self
                .presentation_bytes
                .checked_add(other.presentation_bytes)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MutationPageKey {
    key: MutationKey,
    lane: MutationLane,
    cursor: MutationCursor,
    ordinal: u64,
    prior: MutationIdentity,
}

impl MutationPageKey {
    pub const fn new(
        key: MutationKey,
        lane: MutationLane,
        cursor: MutationCursor,
        ordinal: u64,
        prior: MutationIdentity,
    ) -> Self {
        Self {
            key,
            lane,
            cursor,
            ordinal,
            prior,
        }
    }

    pub const fn key(self) -> MutationKey {
        self.key
    }

    pub const fn lane(self) -> MutationLane {
        self.lane
    }

    pub const fn cursor(self) -> MutationCursor {
        self.cursor
    }

    pub const fn ordinal(self) -> u64 {
        self.ordinal
    }

    pub const fn prior(self) -> MutationIdentity {
        self.prior
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MutationPageItem {
    Utf8 {
        inserted_offset: u64,
        text: Box<str>,
    },
    Atom(AtomChange),
    Object(ObjectChange),
}

impl MutationPageItem {
    fn totals(&self) -> MutationTotals {
        match self {
            Self::Utf8 { text, .. } => MutationTotals {
                inserted_bytes: text.len() as u64,
                inserted_line_breaks: text.bytes().filter(|byte| *byte == b'\n').count() as u64,
                retained_bytes: text.len() as u64,
                ..MutationTotals::default()
            },
            Self::Atom(AtomChange::Insert { fallback_copy, .. }) => MutationTotals {
                retained_bytes: fallback_copy.len() as u64,
                ..MutationTotals::default()
            },
            Self::Atom(AtomChange::Remove { .. }) => MutationTotals::default(),
            Self::Object(change) => {
                let (object_bytes, presentation_bytes) = match change {
                    ObjectChange::Insert { object, .. }
                    | ObjectChange::Replace { object, .. }
                    | ObjectChange::Move { object, .. } => (
                        object.retained_bytes() as u64,
                        object.presentation_bytes() as u64,
                    ),
                    ObjectChange::Remove { .. } => (0, 0),
                };
                MutationTotals {
                    objects: 1,
                    object_bytes,
                    presentation_bytes,
                    ..MutationTotals::default()
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationPage {
    key: MutationPageKey,
    next_cursor: MutationCursor,
    items: std::sync::Arc<[MutationPageItem]>,
    page_identity: MutationIdentity,
    cumulative_identity: MutationIdentity,
    totals: MutationTotals,
}

impl MutationPage {
    pub fn new(
        key: MutationPageKey,
        next_cursor: MutationCursor,
        items: Vec<MutationPageItem>,
    ) -> Result<Self, MutationError> {
        if items.is_empty() || next_cursor <= key.cursor() {
            return Err(MutationError::MalformedPage);
        }
        let mut totals = MutationTotals {
            pages: 1,
            items: items.len() as u64,
            ..MutationTotals::default()
        };
        for item in &items {
            totals = totals
                .checked_add(item.totals())
                .ok_or(MutationError::CumulativeOverflow)?;
        }
        let page_identity = canonical_page_identity(key, next_cursor, &items);
        let cumulative_identity = chain_identity(key.prior(), page_identity, totals);
        Ok(Self {
            key,
            next_cursor,
            items: items.into(),
            page_identity,
            cumulative_identity,
            totals,
        })
    }

    pub const fn key(&self) -> MutationPageKey {
        self.key
    }

    pub const fn next_cursor(&self) -> MutationCursor {
        self.next_cursor
    }

    pub fn items(&self) -> &[MutationPageItem] {
        &self.items
    }

    pub const fn page_identity(&self) -> MutationIdentity {
        self.page_identity
    }

    pub const fn cumulative_identity(&self) -> MutationIdentity {
        self.cumulative_identity
    }

    pub const fn totals(&self) -> MutationTotals {
        self.totals
    }

    pub fn payload_owner_count(&self) -> usize {
        std::sync::Arc::strong_count(&self.items)
    }

    pub(crate) fn payload_allocation_key(&self) -> usize {
        self.items.as_ptr() as usize
    }

    pub(crate) fn payload_allocation_charge(&self) -> Option<(usize, usize)> {
        let mut bytes = self
            .items
            .len()
            .checked_mul(std::mem::size_of::<MutationPageItem>())?;
        let mut retained_items = self.items.len();
        for item in self.items.iter() {
            let (payload_bytes, payload_items) = match item {
                MutationPageItem::Utf8 { text, .. } => (text.len(), 1),
                MutationPageItem::Atom(AtomChange::Insert { fallback_copy, .. }) => {
                    (fallback_copy.len(), 1)
                }
                MutationPageItem::Atom(AtomChange::Remove { .. }) => (0, 0),
                MutationPageItem::Object(change) => match change {
                    ObjectChange::Insert { object, .. }
                    | ObjectChange::Replace { object, .. }
                    | ObjectChange::Move { object, .. } => (
                        object
                            .retained_bytes()
                            .checked_add(object.presentation_bytes())?,
                        1,
                    ),
                    ObjectChange::Remove { .. } => (0, 0),
                },
            };
            bytes = bytes.checked_add(payload_bytes)?;
            retained_items = retained_items.checked_add(payload_items)?;
        }
        Some((bytes, retained_items))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationStreamFinish {
    pub next_cursor: MutationCursor,
    pub next_ordinal: u64,
    pub cumulative_identity: MutationIdentity,
    pub totals: MutationTotals,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationFinishInput {
    key: MutationKey,
    source: MutationStreamFinish,
    proposal: MutationStreamFinish,
    intended_extent: LogicalExtent,
    intended: MutationPositions,
}

impl MutationFinishInput {
    pub const fn new(
        key: MutationKey,
        source: MutationStreamFinish,
        proposal: MutationStreamFinish,
        intended_extent: LogicalExtent,
        intended: MutationPositions,
    ) -> Self {
        Self {
            key,
            source,
            proposal,
            intended_extent,
            intended,
        }
    }

    pub const fn key(self) -> MutationKey {
        self.key
    }

    pub const fn source(self) -> MutationStreamFinish {
        self.source
    }

    pub const fn proposal(self) -> MutationStreamFinish {
        self.proposal
    }

    pub const fn intended_extent(self) -> LogicalExtent {
        self.intended_extent
    }

    pub const fn intended(self) -> MutationPositions {
        self.intended
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationBeginRequest {
    proposal: MutationProposal,
    source_cursor: MutationCursor,
    proposal_cursor: MutationCursor,
}

impl MutationBeginRequest {
    pub const fn new(
        proposal: MutationProposal,
        source_cursor: MutationCursor,
        proposal_cursor: MutationCursor,
    ) -> Self {
        Self {
            proposal,
            source_cursor,
            proposal_cursor,
        }
    }

    pub const fn proposal(self) -> MutationProposal {
        self.proposal
    }

    pub const fn source_cursor(self) -> MutationCursor {
        self.source_cursor
    }

    pub const fn proposal_cursor(self) -> MutationCursor {
        self.proposal_cursor
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationPageRequest {
    page: MutationPage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationPageAcceptance {
    Accepted {
        next_cursor: MutationCursor,
        next_ordinal: u64,
        cumulative_identity: MutationIdentity,
        totals: MutationTotals,
    },
    Replay,
}

impl MutationPageRequest {
    pub const fn new(page: MutationPage) -> Self {
        Self { page }
    }

    pub const fn page(&self) -> &MutationPage {
        &self.page
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationCommitRequest {
    key: MutationKey,
    finish_identity: MutationIdentity,
}

impl MutationCommitRequest {
    pub const fn new(key: MutationKey, finish_identity: MutationIdentity) -> Self {
        Self {
            key,
            finish_identity,
        }
    }

    pub const fn key(self) -> MutationKey {
        self.key
    }

    pub const fn finish_identity(self) -> MutationIdentity {
        self.finish_identity
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationCancelRequest {
    key: MutationKey,
}

impl MutationCancelRequest {
    pub const fn new(key: MutationKey) -> Self {
        Self { key }
    }

    pub const fn key(self) -> MutationKey {
        self.key
    }
}
