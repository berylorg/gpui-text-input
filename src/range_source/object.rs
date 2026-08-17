mod page;
mod presentation;
mod request;

pub use page::*;
pub use presentation::*;
pub use request::*;

macro_rules! opaque_id {
    ($name:ident, $doc:literal) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

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

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ObjectContractError {
    ZeroObjectLimit,
    ZeroRetainedByteLimit,
    DemandOutsideExtent,
    ResponseOutsideEnvelope,
    CursorOutsideEnvelope,
    DuplicateObjectIdentity {
        object: super::InlineObjectId,
    },
    ConflictingObjectIdentity {
        object: super::InlineObjectId,
    },
    ConflictingPageIdentity {
        page: ObjectPageId,
    },
    DuplicateObjectOrder {
        anchor: crate::ByteOffset,
        order: super::InlineObjectOrder,
    },
    ObjectsOutOfOrder,
    MalformedContinuation,
    NonProgressingObjectPage,
    ObjectCountLimitExceeded,
    RetainedByteLimitExceeded,
    InvalidPresentationMetrics,
    RetainedByteCountOverflow,
    ScalarBoundaryProofMismatch {
        anchor: crate::ByteOffset,
    },
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
