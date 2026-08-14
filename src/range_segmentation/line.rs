use super::{AdjacentPageEdge, ByteOffset, SegmentationDirection, SegmentationError};

#[derive(Clone, Copy, Debug)]
pub(super) enum CursorOutcome {
    Boundary(usize),
    DocumentEdge,
    Need(AdjacentPageEdge),
}

pub(super) fn line_outcome(
    direction: SegmentationDirection,
    origin: ByteOffset,
    chunk: &str,
    chunk_start: usize,
) -> Result<CursorOutcome, SegmentationError> {
    match direction {
        SegmentationDirection::Forward => {
            if let Some(relative) = chunk.as_bytes().iter().position(|byte| *byte == b'\n') {
                let boundary = chunk_start
                    .checked_add(relative)
                    .and_then(|offset| offset.checked_add(1))
                    .ok_or(SegmentationError::OffsetOutOfRange)?;
                Ok(CursorOutcome::Boundary(boundary))
            } else {
                let edge = chunk_start
                    .checked_add(chunk.len())
                    .ok_or(SegmentationError::OffsetOutOfRange)?;
                Ok(CursorOutcome::Need(AdjacentPageEdge::NextChunk(
                    ByteOffset::new(
                        u64::try_from(edge).map_err(|_| SegmentationError::OffsetOutOfRange)?,
                    ),
                )))
            }
        }
        SegmentationDirection::Reverse => {
            let mut bytes = chunk.as_bytes();
            loop {
                let Some(relative) = bytes.iter().rposition(|byte| *byte == b'\n') else {
                    return Ok(CursorOutcome::Need(AdjacentPageEdge::PrevChunk(
                        ByteOffset::new(
                            u64::try_from(chunk_start)
                                .map_err(|_| SegmentationError::OffsetOutOfRange)?,
                        ),
                    )));
                };
                let boundary = chunk_start
                    .checked_add(relative)
                    .and_then(|offset| offset.checked_add(1))
                    .ok_or(SegmentationError::OffsetOutOfRange)?;
                if u64::try_from(boundary).map_err(|_| SegmentationError::OffsetOutOfRange)?
                    != origin.get()
                {
                    return Ok(CursorOutcome::Boundary(boundary));
                }
                bytes = &bytes[..relative];
            }
        }
    }
}
