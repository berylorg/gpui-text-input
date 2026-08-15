use std::ops::Range;

use gpui::{Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window};

use crate::{
    ByteOffset, ByteRange, InlineObjectGap, RangeSelection, RangeSourceSelection, RangeSurfaceHit,
    RangeTextInput, SourcePosition,
};

impl EntityInputHandler for RangeTextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        match self.platform_text_for_range(range.clone(), cx) {
            Ok(crate::PlatformRangeResult::Ready(text)) => {
                actual_range.replace(range);
                Some(text)
            }
            Ok(crate::PlatformRangeResult::Pending(_)) | Err(_) => None,
        }
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if !self.enabled && !ignore_disabled_input {
            return None;
        }
        let selection = self.interactive_surface()?.platform_selection()?;
        let anchor = self.resident_utf16_offset(selection.anchor)?;
        let head = self.resident_utf16_offset(selection.head)?;
        Some(UTF16Selection {
            range: anchor.min(head)..anchor.max(head),
            reversed: anchor > head,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let marked = self.interactive_surface()?.composition()?;
        Some(
            self.resident_utf16_offset(marked.start())?
                ..self.resident_utf16_offset(marked.end())?,
        )
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.clear_composition(cx);
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match range {
            Some(range) => {
                let _ = self.replace_platform_range(range, text.to_owned(), cx);
            }
            None => {
                let _ = self.insert_text(text.to_owned(), cx);
            }
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        let replacement = match range.as_ref() {
            Some(range) => self.resident_byte_range(range.clone()),
            None => surface.platform_selection().map(RangeSelection::range),
        };
        let Some(replacement) = replacement else {
            if let Some(range) = range {
                let _ = self.replace_and_mark_platform_range(range, text.to_owned(), selected, cx);
            }
            return;
        };
        let _ = self.begin_marked_replacement(replacement, text.to_owned(), selected, cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.resident_byte_range(range)?;
        let surface = self.interactive_surface()?;
        let (first, last) = surface.first_last_bounds_for_range(
            range,
            self.config.layout.line_height,
            self.config.layout.wrap_width,
        )?;
        Some(Bounds::from_corners(first.origin, last.bottom_right()))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let surface = self.interactive_surface()?;
        let local = point - self.last_origin() + gpui::point(Pixels::ZERO, surface.scroll_block());
        let RangeSurfaceHit::Gap(position) = surface.hit_test_composite(local)? else {
            return None;
        };
        if !matches!(position.gap, InlineObjectGap::NoObjects) {
            return None;
        }
        self.resident_utf16_offset(position.byte_offset)
    }
}

impl RangeTextInput {
    pub(super) fn clear_composition(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<bool, crate::RangeTextInputError> {
        if self.desired.composition.is_none() || self.geometry.index().is_none() {
            return Ok(false);
        }
        let mut desired = self.desired;
        desired.composition = None;
        let candidate = self.prepare_target_transition(desired, None)?;
        self.commit_widget_transition(candidate, Some(cx));
        Ok(true)
    }

    pub(super) fn record_marked_mutation(
        &mut self,
        key: crate::MutationKey,
        replacement: ByteRange,
        text: &str,
        selected: Option<Range<usize>>,
    ) {
        let composition = ByteRange::from_u64(
            replacement.start().get(),
            replacement.start().get().saturating_add(text.len() as u64),
        )
        .expect("inserted range is ordered");
        let selected = selected
            .and_then(|range| utf16_range_in_text(text, range))
            .map(|range| RangeSourceSelection {
                anchor: SourcePosition::new(
                    ByteOffset::new(composition.start().get() + range.start as u64),
                    InlineObjectGap::NoObjects,
                ),
                head: SourcePosition::new(
                    ByteOffset::new(composition.start().get() + range.end as u64),
                    InlineObjectGap::NoObjects,
                ),
            })
            .unwrap_or_else(|| {
                RangeSourceSelection::caret(SourcePosition::new(
                    composition.end(),
                    InlineObjectGap::NoObjects,
                ))
            });
        self.mutation_composition = Some((key, composition, selected));
    }

    fn resident_utf16_offset(&self, offset: ByteOffset) -> Option<usize> {
        if offset.get() == 0 {
            return Some(0);
        }
        let surface = self.interactive_surface()?;
        let mut cursor = ByteOffset::new(0);
        let mut utf16 = 0usize;
        for page in surface.pages_in_source_order() {
            if page.range().start() != cursor {
                return None;
            }
            let end = page.range().end().min(offset);
            let local_end = usize::try_from(end.get() - page.range().start().get()).ok()?;
            if !page.text().is_char_boundary(local_end) {
                return None;
            }
            utf16 = utf16.checked_add(page.text()[..local_end].encode_utf16().count())?;
            cursor = end;
            if cursor == offset {
                return Some(utf16);
            }
        }
        None
    }

    fn resident_byte_range(&self, range: Range<usize>) -> Option<ByteRange> {
        if range.start > range.end {
            return None;
        }
        let surface = self.interactive_surface()?;
        let mut utf16 = 0usize;
        let mut cursor = ByteOffset::new(0);
        let mut start = None;
        let mut end = None;
        for page in surface.pages_in_source_order() {
            if page.range().start() != cursor {
                return None;
            }
            for (local, ch) in page.text().char_indices() {
                let global = page.range().start().get() + local as u64;
                if utf16 == range.start {
                    start = Some(ByteOffset::new(global));
                }
                if utf16 == range.end {
                    end = Some(ByteOffset::new(global));
                }
                utf16 = utf16.checked_add(ch.len_utf16())?;
            }
            cursor = page.range().end();
            if utf16 == range.start {
                start = Some(cursor);
            }
            if utf16 == range.end {
                end = Some(cursor);
            }
            if start.is_some() && end.is_some() {
                break;
            }
        }
        ByteRange::new(start?, end?).ok()
    }
}

fn utf16_range_in_text(text: &str, range: Range<usize>) -> Option<Range<usize>> {
    fn byte_at(text: &str, target: usize) -> Option<usize> {
        let mut utf16 = 0usize;
        for (byte, ch) in text.char_indices() {
            if utf16 == target {
                return Some(byte);
            }
            utf16 = utf16.checked_add(ch.len_utf16())?;
            if utf16 > target {
                return None;
            }
        }
        (utf16 == target).then_some(text.len())
    }
    Some(byte_at(text, range.start)?..byte_at(text, range.end)?)
}
