use super::*;
use crate::{InlineObjectGap, SourcePosition};

#[derive(Clone, Copy, Debug)]
pub(super) struct RemovalTail {
    pub(super) cursor: ObjectCursor,
    pub(super) preceding: Option<ObjectCursor>,
}

pub(super) struct PreparedObjectRemovals {
    pub(super) items: Vec<MutationPageItem>,
    pub(super) cursor: Option<ObjectCursor>,
    pub(super) tail: Option<RemovalTail>,
    pub(super) complete: bool,
}

pub(super) fn prepare_removals(
    producer: &LocalReplacementProducer,
    page: &ObjectPage,
    limit: usize,
) -> Result<PreparedObjectRemovals, RangeTextInputError> {
    let mut items = Vec::with_capacity(limit);
    let leading = producer
        .tail
        .map(|tail| tail.cursor)
        .or_else(|| edge_cursor(page.preceding()));
    if let Some(tail) = producer.tail {
        let following = page.objects().first().map(crate::InlineObjectFact::cursor);
        if following.is_none() && !page.complete() {
            return Err(RangeTextInputError::Stale);
        }
        items.push(MutationPageItem::Object(ObjectChange::Remove {
            target: removal_target(tail, following)?,
        }));
    }
    let mut tail = None;
    let mut reached_end = false;
    for (index, fact) in page.objects().iter().enumerate() {
        let cursor = fact.cursor();
        if !follows_start(cursor, producer.range.start()) {
            continue;
        }
        if !precedes_end(cursor, producer.range.end()) {
            reached_end = true;
            break;
        }
        let preceding = index
            .checked_sub(1)
            .map(|prior| page.objects()[prior].cursor())
            .or(leading);
        let candidate = RemovalTail { cursor, preceding };
        if items.len() == limit {
            tail = Some(candidate);
            break;
        }
        let following = page
            .objects()
            .get(index + 1)
            .map(crate::InlineObjectFact::cursor);
        if following.is_none() && !page.complete() {
            tail = Some(candidate);
            break;
        }
        items.push(MutationPageItem::Object(ObjectChange::Remove {
            target: removal_target(candidate, following)?,
        }));
    }
    let complete = reached_end || (page.complete() && tail.is_none());
    let cursor = if complete {
        None
    } else {
        tail.map(|tail| tail.cursor).or_else(|| page.continuation())
    };
    if !complete && cursor.is_none() {
        return Err(RangeTextInputError::Stale);
    }
    Ok(PreparedObjectRemovals {
        items,
        cursor,
        tail,
        complete,
    })
}

fn edge_cursor(edge: ObjectPageEdgeFact) -> Option<ObjectCursor> {
    match edge {
        ObjectPageEdgeFact::EnvelopeBoundary => None,
        ObjectPageEdgeFact::Continues(cursor) => Some(cursor),
    }
}

fn removal_target(
    tail: RemovalTail,
    following: Option<ObjectCursor>,
) -> Result<ObjectTarget, RangeTextInputError> {
    let current = tail.cursor.neighbor();
    let preceding = tail
        .preceding
        .filter(|cursor| cursor.anchor() == tail.cursor.anchor())
        .map(ObjectCursor::neighbor);
    let following = following
        .filter(|cursor| cursor.anchor() == tail.cursor.anchor())
        .map(ObjectCursor::neighbor);
    let start = if let Some(preceding) = preceding {
        InlineObjectGap::between(preceding, current).map_err(|_| RangeTextInputError::Stale)?
    } else {
        InlineObjectGap::before(current)
    };
    let end = if let Some(following) = following {
        InlineObjectGap::between(current, following).map_err(|_| RangeTextInputError::Stale)?
    } else {
        InlineObjectGap::after(current)
    };
    let range = SourceRange::new(
        SourcePosition::new(tail.cursor.anchor(), start),
        SourcePosition::new(tail.cursor.anchor(), end),
    )
    .map_err(|_| RangeTextInputError::Stale)?;
    ObjectTarget::new(range, current.id(), current.order()).map_err(|_| RangeTextInputError::Stale)
}

fn follows_start(cursor: ObjectCursor, start: SourcePosition) -> bool {
    match cursor.anchor().cmp(&start.byte_offset) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match start.gap {
            InlineObjectGap::NoObjects => false,
            InlineObjectGap::Before(first) => {
                cursor >= ObjectCursor::new(start.byte_offset, first.order(), first.id())
            }
            InlineObjectGap::Between { preceding, .. } | InlineObjectGap::After(preceding) => {
                cursor > ObjectCursor::new(start.byte_offset, preceding.order(), preceding.id())
            }
        },
    }
}

fn precedes_end(cursor: ObjectCursor, end: SourcePosition) -> bool {
    match cursor.anchor().cmp(&end.byte_offset) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => match end.gap {
            InlineObjectGap::NoObjects => false,
            InlineObjectGap::Before(first) => {
                cursor < ObjectCursor::new(end.byte_offset, first.order(), first.id())
            }
            InlineObjectGap::Between { preceding, .. } | InlineObjectGap::After(preceding) => {
                cursor <= ObjectCursor::new(end.byte_offset, preceding.order(), preceding.id())
            }
        },
    }
}
