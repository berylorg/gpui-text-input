use super::{ClipboardKey, storage::ExactArray};
use crate::{
    ByteOffset, ByteRange, InlineObjectFact, InlineObjectId, InlineObjectOrder, ObjectCursor,
};
use std::{mem::size_of, sync::Arc};

mod identity;

use identity::{canonical_cumulative_identity, canonical_final_identity, canonical_page_identity};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardProvenanceLimits {
    max_page_items: usize,
    max_page_retained_bytes: usize,
}

impl ClipboardProvenanceLimits {
    pub fn new(
        max_page_items: usize,
        max_page_retained_bytes: usize,
    ) -> Result<Self, super::ClipboardError> {
        if max_page_items == 0
            || u32::try_from(max_page_items).is_err()
            || u32::try_from(max_page_retained_bytes).is_err()
            || page_retained_bytes(max_page_items)
                .is_none_or(|bytes| bytes > max_page_retained_bytes)
        {
            return Err(super::ClipboardError::InvalidLimits);
        }
        Ok(Self {
            max_page_items,
            max_page_retained_bytes,
        })
    }

    pub const fn max_page_items(self) -> usize {
        self.max_page_items
    }

    pub const fn max_page_retained_bytes(self) -> usize {
        self.max_page_retained_bytes
    }

    pub(super) const fn from_valid(max_page_items: usize, max_page_retained_bytes: usize) -> Self {
        Self {
            max_page_items,
            max_page_retained_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ClipboardProvenancePolicy {
    #[default]
    Omit,
    Stream(ClipboardProvenanceLimits),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardProvenanceItem {
    object_id: InlineObjectId,
    source_anchor: ByteOffset,
    source_order: InlineObjectOrder,
    output_range: ByteRange,
}

impl ClipboardProvenanceItem {
    pub const fn object_id(self) -> InlineObjectId {
        self.object_id
    }

    pub const fn source_anchor(self) -> ByteOffset {
        self.source_anchor
    }

    pub const fn source_order(self) -> InlineObjectOrder {
        self.source_order
    }

    pub const fn output_range(self) -> ByteRange {
        self.output_range
    }

    pub(super) fn from_object(
        object: &InlineObjectFact,
        output_start: usize,
        output_end: usize,
    ) -> Option<Self> {
        let start = ByteOffset::new(u64::try_from(output_start).ok()?);
        let end = ByteOffset::new(u64::try_from(output_end).ok()?);
        Some(Self {
            object_id: object.id(),
            source_anchor: object.anchor(),
            source_order: object.order(),
            output_range: ByteRange::new(start, end).ok()?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardProvenanceCursor {
    preceding_object: Option<ObjectCursor>,
    item_ordinal: u64,
    output_offset: ByteOffset,
}

impl ClipboardProvenanceCursor {
    pub const fn preceding_object(self) -> Option<ObjectCursor> {
        self.preceding_object
    }

    pub const fn item_ordinal(self) -> u64 {
        self.item_ordinal
    }

    pub const fn output_offset(self) -> ByteOffset {
        self.output_offset
    }

    pub(super) const fn new(
        preceding_object: Option<ObjectCursor>,
        item_ordinal: u64,
        output_offset: ByteOffset,
    ) -> Self {
        Self {
            preceding_object,
            item_ordinal,
            output_offset,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClipboardProvenanceIdentity([u64; 4]);

impl ClipboardProvenanceIdentity {
    pub const ROOT: Self = Self([
        0x243f_6a88_85a3_08d3,
        0x1319_8a2e_0370_7344,
        0xa409_3822_299f_31d0,
        0x082e_fa98_ec4e_6c89,
    ]);

    const fn words(self) -> [u64; 4] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardProvenancePageKey {
    clipboard: ClipboardKey,
    page_ordinal: u64,
}

impl ClipboardProvenancePageKey {
    pub const fn clipboard(self) -> ClipboardKey {
        self.clipboard
    }

    pub const fn page_ordinal(self) -> u64 {
        self.page_ordinal
    }
}

#[derive(Debug, Eq, PartialEq)]
struct ClipboardProvenancePageData {
    key: ClipboardProvenancePageKey,
    cursor: ClipboardProvenanceCursor,
    prior_identity: ClipboardProvenanceIdentity,
    next_cursor: ClipboardProvenanceCursor,
    page_identity: ClipboardProvenanceIdentity,
    cumulative_identity: ClipboardProvenanceIdentity,
    retained_bytes: usize,
    items: ExactArray<ClipboardProvenanceItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardProvenancePage(Arc<ClipboardProvenancePageData>);

impl ClipboardProvenancePage {
    pub fn key(&self) -> ClipboardProvenancePageKey {
        self.0.key
    }

    pub fn next_cursor(&self) -> ClipboardProvenanceCursor {
        self.0.next_cursor
    }

    pub fn cursor(&self) -> ClipboardProvenanceCursor {
        self.0.cursor
    }

    pub fn prior_identity(&self) -> ClipboardProvenanceIdentity {
        self.0.prior_identity
    }

    pub fn page_identity(&self) -> ClipboardProvenanceIdentity {
        self.0.page_identity
    }

    pub fn cumulative_identity(&self) -> ClipboardProvenanceIdentity {
        self.0.cumulative_identity
    }

    pub fn retained_bytes(&self) -> usize {
        self.0.retained_bytes
    }

    pub fn items(&self) -> &[ClipboardProvenanceItem] {
        self.0.items.as_slice()
    }

    pub(crate) fn payload_allocation_charge(&self) -> (usize, usize) {
        (
            page_shared_allocation_bytes(self.0.items.capacity())
                .expect("validated provenance page allocation fits usize"),
            self.0
                .items
                .capacity()
                .checked_add(1)
                .expect("validated provenance page items fit usize"),
        )
    }

    pub(crate) fn shares_allocation_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardProvenanceClosure {
    page_count: u64,
    item_count: u64,
    fallback_bytes: u64,
    output_bytes: u64,
    prior_identity: ClipboardProvenanceIdentity,
    final_identity: ClipboardProvenanceIdentity,
}

impl ClipboardProvenanceClosure {
    pub const fn page_count(self) -> u64 {
        self.page_count
    }

    pub const fn item_count(self) -> u64 {
        self.item_count
    }

    pub const fn fallback_bytes(self) -> u64 {
        self.fallback_bytes
    }

    pub const fn output_bytes(self) -> u64 {
        self.output_bytes
    }

    pub const fn prior_identity(self) -> ClipboardProvenanceIdentity {
        self.prior_identity
    }

    pub const fn final_identity(self) -> ClipboardProvenanceIdentity {
        self.final_identity
    }
}

#[derive(Debug)]
pub(super) struct ProvenanceCollection {
    pub limits: ClipboardProvenanceLimits,
    pub items: Option<ExactArray<ClipboardProvenanceItem>>,
    pub page_start: Option<ClipboardProvenanceCursor>,
    pub preceding_object: Option<ObjectCursor>,
    pub page_ordinal: u64,
    pub item_count: u64,
    pub fallback_bytes: u64,
    pub cumulative_identity: ClipboardProvenanceIdentity,
    pub current_page: Option<ClipboardProvenancePage>,
}

impl ProvenanceCollection {
    pub fn new(limits: ClipboardProvenanceLimits) -> Self {
        Self {
            limits,
            items: None,
            page_start: None,
            preceding_object: None,
            page_ordinal: 0,
            item_count: 0,
            fallback_bytes: 0,
            cumulative_identity: ClipboardProvenanceIdentity::ROOT,
            current_page: None,
        }
    }

    pub fn builder_allocation_charge(&self) -> Option<(usize, usize)> {
        Some((
            self.limits
                .max_page_items
                .checked_mul(size_of::<ClipboardProvenanceItem>())?,
            self.limits.max_page_items,
        ))
    }

    pub fn allocate_builder(&mut self) -> Result<(), ()> {
        if self.current_page.is_some() || self.items.is_some() {
            return Err(());
        }
        self.items = Some(ExactArray::try_with_capacity(self.limits.max_page_items)?);
        Ok(())
    }

    pub fn builder_is_full(&self) -> bool {
        self.items
            .as_ref()
            .is_some_and(|items| items.len() == self.limits.max_page_items)
    }

    pub fn emitted_ownership_charge(&self) -> Option<(usize, usize)> {
        let items = self.items.as_ref()?;
        if items.is_empty() || items.capacity() != self.limits.max_page_items {
            return None;
        }
        Some((
            size_of::<Self>().checked_add(page_shared_allocation_bytes(items.capacity())?)?,
            1usize.checked_add(items.capacity())?.checked_add(1)?,
        ))
    }

    pub fn push(
        &mut self,
        object: &InlineObjectFact,
        output_start: usize,
        output_end: usize,
    ) -> Result<bool, ()> {
        let item =
            ClipboardProvenanceItem::from_object(object, output_start, output_end).ok_or(())?;
        let fallback_bytes =
            u64::try_from(output_end.checked_sub(output_start).ok_or(())?).map_err(|_| ())?;
        let next_count = self.item_count.checked_add(1).ok_or(())?;
        let next_fallback = self.fallback_bytes.checked_add(fallback_bytes).ok_or(())?;
        let output_start = ByteOffset::new(u64::try_from(output_start).map_err(|_| ())?);
        if self.items.is_none() {
            return Err(());
        }
        if self.page_start.is_none() {
            self.page_start = Some(ClipboardProvenanceCursor::new(
                self.preceding_object,
                self.item_count,
                output_start,
            ));
        }
        let items = self.items.as_mut().ok_or(())?;
        if items.len() == self.limits.max_page_items {
            return Err(());
        }
        items.push(item).map_err(|_| ())?;
        self.item_count = next_count;
        self.fallback_bytes = next_fallback;
        self.preceding_object = Some(object.cursor());
        Ok(items.len() == self.limits.max_page_items)
    }

    pub fn has_items(&self) -> bool {
        self.items.as_ref().is_some_and(|items| !items.is_empty())
    }

    pub fn emit(&mut self, clipboard: ClipboardKey) -> Result<ClipboardProvenancePage, ()> {
        if self.current_page.is_some() {
            return Err(());
        }
        let items = self.items.take().ok_or(())?;
        if items.is_empty() || items.capacity() != self.limits.max_page_items {
            return Err(());
        }
        let start = self.page_start.take().ok_or(())?;
        let last = items.as_slice().last().ok_or(())?;
        let next = ClipboardProvenanceCursor::new(
            self.preceding_object,
            self.item_count,
            last.output_range().end(),
        );
        let key = ClipboardProvenancePageKey {
            clipboard,
            page_ordinal: self.page_ordinal,
        };
        let prior_identity = self.cumulative_identity;
        let page_identity =
            canonical_page_identity(key, start, prior_identity, next, items.as_slice());
        let cumulative_identity = canonical_cumulative_identity(
            self.cumulative_identity,
            page_identity,
            next,
            self.item_count,
            self.fallback_bytes,
        );
        let retained_bytes = page_retained_bytes(items.capacity()).ok_or(())?;
        if retained_bytes > self.limits.max_page_retained_bytes {
            return Err(());
        }
        let page = ClipboardProvenancePage(Arc::new(ClipboardProvenancePageData {
            key,
            cursor: start,
            prior_identity,
            next_cursor: next,
            page_identity,
            cumulative_identity,
            retained_bytes,
            items,
        }));
        self.current_page = Some(page.clone());
        Ok(page)
    }

    pub fn acknowledge(&mut self, page: ClipboardProvenancePage) -> Result<(), bool> {
        let (cumulative_identity, page_ordinal) = {
            let Some(current) = self.current_page.as_ref() else {
                return Err(false);
            };
            if page.key() != current.key() {
                return Err(false);
            }
            if page != *current {
                return Err(true);
            }
            (
                current.cumulative_identity(),
                self.page_ordinal.checked_add(1).ok_or(true)?,
            )
        };
        self.cumulative_identity = cumulative_identity;
        self.page_ordinal = page_ordinal;
        self.current_page = None;
        drop(page);
        debug_assert!(self.items.is_none());
        Ok(())
    }

    pub fn closure(
        &self,
        clipboard: ClipboardKey,
        text: &str,
    ) -> Result<ClipboardProvenanceClosure, ()> {
        if self.current_page.is_some() || self.has_items() {
            return Err(());
        }
        let output_bytes = u64::try_from(text.len()).map_err(|_| ())?;
        let final_identity = canonical_final_identity(
            clipboard,
            self.page_ordinal,
            self.item_count,
            self.fallback_bytes,
            output_bytes,
            self.cumulative_identity,
            text.as_bytes(),
        );
        Ok(ClipboardProvenanceClosure {
            page_count: self.page_ordinal,
            item_count: self.item_count,
            fallback_bytes: self.fallback_bytes,
            output_bytes,
            prior_identity: self.cumulative_identity,
            final_identity,
        })
    }

    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.current_page.as_ref().map_or_else(
            || {
                self.items.as_ref().map_or(0, |items| {
                    items
                        .capacity()
                        .checked_mul(size_of::<ClipboardProvenanceItem>())
                        .unwrap_or(usize::MAX)
                })
            },
            ClipboardProvenancePage::retained_bytes,
        ))
    }

    pub fn ownership_charge(&self) -> Option<(usize, usize)> {
        let (payload_bytes, payload_items) = match self.current_page.as_ref() {
            Some(page) => page.payload_allocation_charge(),
            None => match self.items.as_ref() {
                Some(items) => (
                    items
                        .capacity()
                        .checked_mul(size_of::<ClipboardProvenanceItem>())?,
                    items.capacity(),
                ),
                None => (0, 0),
            },
        };
        Some((
            size_of::<Self>().checked_add(payload_bytes)?,
            1usize.checked_add(payload_items)?,
        ))
    }
}

fn page_retained_bytes(capacity: usize) -> Option<usize> {
    page_shared_allocation_bytes(capacity)?
        .checked_add(2usize.checked_mul(size_of::<ClipboardProvenancePage>())?)
}

fn page_shared_allocation_bytes(capacity: usize) -> Option<usize> {
    size_of::<ClipboardProvenancePageData>()
        .checked_add(2usize.checked_mul(size_of::<usize>())?)?
        .checked_add(capacity.checked_mul(size_of::<ClipboardProvenanceItem>())?)
}
