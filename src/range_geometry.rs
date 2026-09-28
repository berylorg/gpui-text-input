//! Exact revision- and epoch-keyed streaming geometry ownership.

mod exact;
mod types;
#[cfg(feature = "test-support")]
pub use exact::preparation_test_support;

pub(crate) use exact::{
    PreparedGeometryTransition, PreparedTargetResponse, PreparedTargetSuccessor,
    PreparedTerminalGeometryFailure, TargetInlineObjectPresentation, TargetResponseSuccessor,
};

pub use exact::{
    BlockTarget, BlockTargetPublication, ExactGeometryAdmission, ExactGeometryAggregate,
    ExactGeometryCheckpoint, ExactGeometryCounts, ExactGeometryError, ExactGeometryFailure,
    ExactGeometryFailureStage, ExactGeometryIndex, ExactGeometryLimits, ExactGeometryOwner,
    ExactGeometryProgress, ExactGeometryRelease, ExactGeometryStart, StreamingGeometryEstimate,
    StreamingGeometryStyle, StreamingOversizePresentation,
};
pub use types::{GeometryJobId, GeometryJobKey, GeometryKey, GeometryQuality, LayoutEpoch};
