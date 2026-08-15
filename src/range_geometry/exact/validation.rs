use gpui::Pixels;

use super::{ExactGeometryError, StreamingGeometryStyle};

pub(super) fn validate_inputs(
    layout: &gpui::StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
) -> Result<(), ExactGeometryError> {
    let limits = layout.limits;
    if layout.start_position.byte_offset != 0
        || crate::SourcePosition::try_from(layout.start_position).is_err()
        || limits.segment_bytes == 0
        || limits.runs == 0
        || limits.decorations == 0
        || limits.glyphs == 0
        || limits.wraps == 0
        || limits.maps == 0
        || limits.fragments == 0
        || limits.retained_items == 0
        || limits.retained_bytes == 0
    {
        return Err(ExactGeometryError::InvalidLimits);
    }
    let metrics = [
        layout.wrap_width,
        layout.font_size,
        layout.line_height,
        style.oversize.height,
    ];
    if metrics
        .iter()
        .any(|value| !f32::from(*value).is_finite() || *value <= Pixels::ZERO)
        || !f32::from(style.oversize.width).is_finite()
        || style.oversize.width < Pixels::ZERO
        || !f32::from(style.oversize.baseline).is_finite()
        || style.oversize.baseline < Pixels::ZERO
        || style.oversize.baseline > style.oversize.height
        || style.oversize.presentation.len() > limits.segment_bytes
        || style.oversize.runs.len() > limits.runs
    {
        return Err(ExactGeometryError::InvalidMetric);
    }
    let run_bytes = style
        .oversize
        .runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.len))
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    if run_bytes != style.oversize.presentation.len() {
        return Err(ExactGeometryError::SourceContract);
    }
    Ok(())
}
