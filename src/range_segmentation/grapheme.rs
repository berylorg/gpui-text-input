use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

use super::{
    AdjacentPageEdge, ByteOffset, CursorOutcome, RangePage, SegmentationDirection,
    SegmentationError,
};
#[derive(Clone, Debug)]
enum PageUse {
    Traverse,
    Context { replay: ByteOffset },
}

#[derive(Clone, Debug)]
pub(super) struct GraphemeEngine {
    cursor: GraphemeCursor,
    page_use: PageUse,
}

impl GraphemeEngine {
    pub(super) fn new(offset: usize, len: usize) -> Self {
        Self {
            cursor: GraphemeCursor::new(offset, len, true),
            page_use: PageUse::Traverse,
        }
    }

    pub(super) fn cursor(&self) -> usize {
        self.cursor.cur_cursor()
    }

    pub(super) fn resume(
        &mut self,
        page: &RangePage,
        direction: SegmentationDirection,
        origin: usize,
        max_steps: usize,
    ) -> Result<CursorOutcome, SegmentationError> {
        let chunk_start = usize::try_from(page.range().start().get())
            .map_err(|_| SegmentationError::OffsetOutOfRange)?;
        if let PageUse::Context { replay } = self.page_use {
            self.cursor.provide_context(page.text(), chunk_start);
            self.page_use = PageUse::Traverse;
            return Ok(CursorOutcome::Need(AdjacentPageEdge::Replay(replay)));
        }

        for _ in 0..max_steps {
            let result = match direction {
                SegmentationDirection::Forward => {
                    self.cursor.next_boundary(page.text(), chunk_start)
                }
                SegmentationDirection::Reverse => {
                    self.cursor.prev_boundary(page.text(), chunk_start)
                }
            };
            match result {
                Ok(Some(boundary)) if boundary == origin => continue,
                Ok(Some(boundary)) => {
                    let valid = match direction {
                        SegmentationDirection::Forward => boundary > origin,
                        SegmentationDirection::Reverse => boundary < origin,
                    };
                    if !valid {
                        return Err(SegmentationError::CursorContract);
                    }
                    return Ok(CursorOutcome::Boundary(boundary));
                }
                Ok(None) => return Ok(CursorOutcome::DocumentEdge),
                Err(GraphemeIncomplete::NextChunk) => {
                    return Ok(CursorOutcome::Need(AdjacentPageEdge::NextChunk(
                        ByteOffset::new(self.cursor.cur_cursor() as u64),
                    )));
                }
                Err(GraphemeIncomplete::PrevChunk) => {
                    return Ok(CursorOutcome::Need(AdjacentPageEdge::PrevChunk(
                        ByteOffset::new(self.cursor.cur_cursor() as u64),
                    )));
                }
                Err(GraphemeIncomplete::PreContext(offset)) if offset > chunk_start => {
                    let context_end = offset - chunk_start;
                    if context_end > page.text().len() || !page.text().is_char_boundary(context_end)
                    {
                        return Err(SegmentationError::CursorContract);
                    }
                    self.cursor
                        .provide_context(&page.text()[..context_end], chunk_start);
                }
                Err(GraphemeIncomplete::PreContext(offset)) => {
                    let replay = match direction {
                        SegmentationDirection::Forward => page.range().start(),
                        SegmentationDirection::Reverse => page.range().end(),
                    };
                    self.page_use = PageUse::Context { replay };
                    return Ok(CursorOutcome::Need(AdjacentPageEdge::PreContext(
                        ByteOffset::new(offset as u64),
                    )));
                }
                Err(GraphemeIncomplete::InvalidOffset) => {
                    return Err(SegmentationError::CursorContract);
                }
            }
        }

        Ok(CursorOutcome::Need(AdjacentPageEdge::Replay(
            match direction {
                SegmentationDirection::Forward => page.range().start(),
                SegmentationDirection::Reverse => page.range().end(),
            },
        )))
    }
}
