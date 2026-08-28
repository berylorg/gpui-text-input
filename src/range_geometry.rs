//! Exact revision- and epoch-keyed streaming geometry ownership.

mod exact;
mod types;

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
