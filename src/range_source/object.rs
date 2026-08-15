//! Separate bounded source protocol for source-zero-width inline objects.

mod page;
mod presentation;
mod request;

pub use page::*;
pub use presentation::*;
pub use request::*;

macro_rules! opaque_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            /// Wraps a host-assigned opaque value.
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the opaque value for host-side correlation.
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

opaque_id!(
    PresentationGeneration,
    "Opaque generation of immutable inline-object presentation facts."
);
opaque_id!(
    ObjectRequestId,
    "Unique identity of one bounded inline-object request."
);
opaque_id!(ObjectPageId, "Stable identity of one object-page payload.");

/// Malformed object-source input rejected at the public boundary.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ObjectContractError {
    /// A request must retain at least one object.
    ZeroObjectLimit,
    /// A request must have a positive retained-byte ceiling.
    ZeroRetainedByteLimit,
    /// A request interval, anchor, or cursor exceeds the bound source extent.
    DemandOutsideExtent,
    /// A response used another demand envelope or returned objects outside its envelope.
    ResponseOutsideEnvelope,
    /// A cursor does not belong to its exact demand anchor or interval.
    CursorOutsideEnvelope,
    /// A page repeated one object identity.
    DuplicateObjectIdentity { object: super::InlineObjectId },
    /// Resident pages disagreed about facts for one stable identity.
    ConflictingObjectIdentity { object: super::InlineObjectId },
    /// Two different page payloads reused one stable page-payload identity.
    ConflictingPageIdentity { page: ObjectPageId },
    /// Two objects at one anchor repeated an order key.
    DuplicateObjectOrder {
        anchor: crate::ByteOffset,
        order: super::InlineObjectOrder,
    },
    /// Returned objects were not in strict `(anchor, order, id)` order.
    ObjectsOutOfOrder,
    /// The response continuation or envelope-edge facts were inconsistent.
    MalformedContinuation,
    /// A nonterminal response made no object progress.
    NonProgressingObjectPage,
    /// A page exceeded its request's object-count ceiling.
    ObjectCountLimitExceeded,
    /// A page exceeded its request's retained-byte ceiling.
    RetainedByteLimitExceeded,
    /// Presentation metrics were invalid.
    InvalidPresentationMetrics,
    /// Exact retained-byte accounting overflowed.
    RetainedByteCountOverflow,
    /// Admission did not receive exactly one matching text-owned proof per distinct object anchor.
    ScalarBoundaryProofMismatch { anchor: crate::ByteOffset },
}

impl std::fmt::Display for ObjectContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "malformed inline-object source contract: {self:?}"
        )
    }
}

impl std::error::Error for ObjectContractError {}
