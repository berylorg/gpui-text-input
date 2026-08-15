use std::mem::size_of;

use crate::range_source::{ByteOffset, InlineObjectId, InlineObjectOrder};

use super::{
    InlineObjectPresentation, ObjectContractError, ObjectCursor, ObjectDirection, ObjectPageId,
    ObjectRequestKey,
};

/// One immutable source-zero-width object fact returned by the host.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineObjectFact {
    id: InlineObjectId,
    anchor: ByteOffset,
    order: InlineObjectOrder,
    fallback_copy: String,
    presentation: InlineObjectPresentation,
}

impl InlineObjectFact {
    /// Creates one revision-bound object fact. Page construction validates its envelope and order.
    pub fn new(
        id: InlineObjectId,
        anchor: ByteOffset,
        order: InlineObjectOrder,
        fallback_copy: impl Into<String>,
        presentation: InlineObjectPresentation,
    ) -> Self {
        Self {
            id,
            anchor,
            order,
            fallback_copy: fallback_copy.into(),
            presentation,
        }
    }

    /// Returns the stable opaque object identity.
    pub const fn id(&self) -> InlineObjectId {
        self.id
    }

    /// Returns the proven UTF-8 anchor.
    pub const fn anchor(&self) -> ByteOffset {
        self.anchor
    }

    /// Returns the host-owned same-anchor order key.
    pub const fn order(&self) -> InlineObjectOrder {
        self.order
    }

    /// Returns the plain-text clipboard fallback.
    pub fn fallback_copy(&self) -> &str {
        &self.fallback_copy
    }

    /// Returns immutable visual, semantic, and activation facts.
    pub const fn presentation(&self) -> &InlineObjectPresentation {
        &self.presentation
    }

    /// Returns the exact continuation cursor for this occurrence.
    pub const fn cursor(&self) -> ObjectCursor {
        ObjectCursor::new(self.anchor, self.order, self.id)
    }

    pub(super) fn payload_bytes(&self) -> Result<usize, ObjectContractError> {
        self.fallback_copy
            .len()
            .checked_add(self.presentation.payload_bytes()?)
            .ok_or(ObjectContractError::RetainedByteCountOverflow)
    }

    pub(crate) fn reconciles_with(&self, other: &Self) -> bool {
        self == other
    }

    pub(crate) fn retained_bytes(&self) -> Result<usize, ObjectContractError> {
        size_of::<Self>()
            .checked_add(self.payload_bytes()?)
            .ok_or(ObjectContractError::RetainedByteCountOverflow)
    }
}

/// Exact fact about one edge of a returned object page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectPageEdgeFact {
    /// The response reaches the applicable request-envelope edge.
    EnvelopeBoundary,
    /// More ordering context is separated by the named exclusive cursor.
    Continues(ObjectCursor),
}

/// Exact semantic charge for one retained object-page graph.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ObjectPageCharge {
    bytes: usize,
    objects: usize,
    presentation_bytes: usize,
}

impl ObjectPageCharge {
    /// Complete initialized records and retained string payload bytes.
    pub const fn bytes(self) -> usize {
        self.bytes
    }

    /// Number of object facts.
    pub const fn objects(self) -> usize {
        self.objects
    }

    /// Display and fallback string bytes.
    pub const fn presentation_bytes(self) -> usize {
        self.presentation_bytes
    }
}

/// One exact bounded source-zero-width object page.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectPage {
    id: ObjectPageId,
    key: ObjectRequestKey,
    objects: Vec<InlineObjectFact>,
    preceding: ObjectPageEdgeFact,
    following: ObjectPageEdgeFact,
    complete: bool,
    continuation: Option<ObjectCursor>,
    charge: ObjectPageCharge,
}

impl ObjectPage {
    /// Constructs and validates one exact response to its frozen object request.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ObjectPageId,
        key: ObjectRequestKey,
        objects: Vec<InlineObjectFact>,
        preceding: ObjectPageEdgeFact,
        following: ObjectPageEdgeFact,
        complete: bool,
        continuation: Option<ObjectCursor>,
    ) -> Result<Self, ObjectContractError> {
        let demand = key.demand();
        demand.validate_local()?;
        if objects.len() > demand.max_objects() {
            return Err(ObjectContractError::ObjectCountLimitExceeded);
        }

        for (index, object) in objects.iter().enumerate() {
            if !demand.contains_anchor(object.anchor()) {
                return Err(ObjectContractError::ResponseOutsideEnvelope);
            }
            if let Some(cursor) = demand.cursor() {
                if object.id() == cursor.id() {
                    return Err(ObjectContractError::DuplicateObjectIdentity {
                        object: object.id(),
                    });
                }
                if object.anchor() == cursor.anchor() && object.order() == cursor.order() {
                    return Err(ObjectContractError::DuplicateObjectOrder {
                        anchor: object.anchor(),
                        order: object.order(),
                    });
                }
                let progresses = match demand.direction() {
                    ObjectDirection::Forward => object.cursor() > cursor,
                    ObjectDirection::Backward => object.cursor() < cursor,
                };
                if !progresses {
                    return Err(ObjectContractError::ResponseOutsideEnvelope);
                }
            }
            if objects[..index]
                .iter()
                .any(|prior| prior.id() == object.id())
            {
                return Err(ObjectContractError::DuplicateObjectIdentity {
                    object: object.id(),
                });
            }
            if let Some(previous) = index.checked_sub(1).map(|prior| &objects[prior]) {
                if previous.anchor() == object.anchor() && previous.order() == object.order() {
                    return Err(ObjectContractError::DuplicateObjectOrder {
                        anchor: object.anchor(),
                        order: object.order(),
                    });
                }
                if previous.cursor() >= object.cursor() {
                    return Err(ObjectContractError::ObjectsOutOfOrder);
                }
            }
        }

        validate_continuation(
            demand.direction(),
            demand.cursor(),
            &objects,
            preceding,
            following,
            complete,
            continuation,
        )?;

        let presentation_bytes = objects.iter().try_fold(0usize, |bytes, object| {
            bytes
                .checked_add(object.payload_bytes()?)
                .ok_or(ObjectContractError::RetainedByteCountOverflow)
        })?;
        let bytes = size_of::<Self>()
            .checked_add(
                objects
                    .len()
                    .checked_mul(size_of::<InlineObjectFact>())
                    .ok_or(ObjectContractError::RetainedByteCountOverflow)?,
            )
            .and_then(|bytes| bytes.checked_add(presentation_bytes))
            .ok_or(ObjectContractError::RetainedByteCountOverflow)?;
        if bytes > demand.max_retained_bytes() {
            return Err(ObjectContractError::RetainedByteLimitExceeded);
        }
        let charge = ObjectPageCharge {
            bytes,
            objects: objects.len(),
            presentation_bytes,
        };
        Ok(Self {
            id,
            key,
            objects,
            preceding,
            following,
            complete,
            continuation,
            charge,
        })
    }

    /// Returns the stable page-payload identity.
    pub const fn id(&self) -> ObjectPageId {
        self.id
    }

    /// Returns the exact request/response key.
    pub const fn key(&self) -> ObjectRequestKey {
        self.key
    }

    /// Returns strictly source-ordered object facts.
    pub fn objects(&self) -> &[InlineObjectFact] {
        &self.objects
    }

    /// Returns the leading envelope or continuation fact.
    pub const fn preceding(&self) -> ObjectPageEdgeFact {
        self.preceding
    }

    /// Returns the trailing envelope or continuation fact.
    pub const fn following(&self) -> ObjectPageEdgeFact {
        self.following
    }

    /// Reports whether the requested direction reaches its envelope edge.
    pub const fn complete(&self) -> bool {
        self.complete
    }

    /// Returns the exclusive cursor for the next page when incomplete.
    pub const fn continuation(&self) -> Option<ObjectCursor> {
        self.continuation
    }

    /// Returns exact retained record and payload accounting.
    pub const fn retained_charge(&self) -> ObjectPageCharge {
        self.charge
    }

    /// Reports whether the same stable page identity names exactly the same payload and facts.
    ///
    /// The request key is response provenance rather than page payload identity, so equivalent
    /// responses to another exact request reconcile when every retained fact and charge matches.
    pub(crate) fn reconciles_with(&self, other: &Self) -> bool {
        self.id == other.id
            && self.objects == other.objects
            && self.preceding == other.preceding
            && self.following == other.following
            && self.complete == other.complete
            && self.continuation == other.continuation
            && self.charge == other.charge
    }
}

fn validate_continuation(
    direction: ObjectDirection,
    request_cursor: Option<ObjectCursor>,
    objects: &[InlineObjectFact],
    preceding: ObjectPageEdgeFact,
    following: ObjectPageEdgeFact,
    complete: bool,
    continuation: Option<ObjectCursor>,
) -> Result<(), ObjectContractError> {
    let request_edge = request_cursor.map_or(
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::Continues,
    );
    let expected_continuation = if complete {
        None
    } else {
        match direction {
            ObjectDirection::Forward => objects.last().map(InlineObjectFact::cursor),
            ObjectDirection::Backward => objects.first().map(InlineObjectFact::cursor),
        }
    };
    if !complete && objects.is_empty() {
        return Err(ObjectContractError::NonProgressingObjectPage);
    }
    if continuation != expected_continuation || complete != continuation.is_none() {
        return Err(ObjectContractError::MalformedContinuation);
    }
    let continuation_edge = continuation.map_or(
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::Continues,
    );
    let edges_valid = match direction {
        ObjectDirection::Forward => preceding == request_edge && following == continuation_edge,
        ObjectDirection::Backward => following == request_edge && preceding == continuation_edge,
    };
    if !edges_valid {
        return Err(ObjectContractError::MalformedContinuation);
    }
    Ok(())
}

/// Terminal host-side failure for one exact object request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectPageFailure {
    /// The exact request was cancelled before success.
    Cancelled,
    /// The exact requested object facts are currently unavailable.
    Unavailable,
    /// The host response could not satisfy the exact contract.
    Malformed,
}
