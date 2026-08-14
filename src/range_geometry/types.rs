use crate::{BindingId, SourceRevision};

macro_rules! opaque_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            /// Wraps an opaque monotonic value.
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
    LayoutEpoch,
    "Monotonic identity of one geometry-affecting layout configuration."
);
opaque_id!(
    GeometryJobId,
    "Unique monotonic identity of one exact geometry job."
);

/// Exact layout identity shared by every exact job and accepted result.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GeometryKey {
    binding: BindingId,
    revision: SourceRevision,
    epoch: LayoutEpoch,
}

impl GeometryKey {
    pub const fn new(binding: BindingId, revision: SourceRevision, epoch: LayoutEpoch) -> Self {
        Self {
            binding,
            revision,
            epoch,
        }
    }

    pub const fn binding(self) -> BindingId {
        self.binding
    }

    pub const fn revision(self) -> SourceRevision {
        self.revision
    }

    pub const fn epoch(self) -> LayoutEpoch {
        self.epoch
    }
}

/// Exact immutable identity of one geometry job.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GeometryJobKey {
    geometry: GeometryKey,
    job: GeometryJobId,
}

impl GeometryJobKey {
    pub const fn new(geometry: GeometryKey, job: GeometryJobId) -> Self {
        Self { geometry, job }
    }

    pub const fn geometry(self) -> GeometryKey {
        self.geometry
    }

    pub const fn job(self) -> GeometryJobId {
        self.job
    }
}

/// Whether published aggregate geometry is estimated or exact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryQuality {
    Estimated,
    Exact,
}
