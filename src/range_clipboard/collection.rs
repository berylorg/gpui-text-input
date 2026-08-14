use super::*;

impl RangeClipboardCoordinator {
    /// Admits one exact page after enforcing its retained payload bound.
    pub fn admit_page(&mut self, page: RangePage) -> Result<ClipboardProgress, ClipboardError> {
        let actual = page.key();
        let Some(active) = &self.active else {
            return Err(ClipboardError::ObsoletePage(actual));
        };
        let Some(expected) = active.pending else {
            return Err(ClipboardError::WrongState {
                expected: ClipboardState::PagePending,
                actual: active.state,
            });
        };
        if actual != expected {
            return Err(ClipboardError::WrongPageKey { expected, actual });
        }
        let retained_too_large = u64::try_from(page.retained_bytes())
            .map_or(true, |bytes| bytes > self.limits.max_page_bytes);
        if retained_too_large {
            let key = active.key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(
                ClipboardCompletion::PageTooLarge,
            ));
        }
        let extent = self.binding.extent().byte_len();
        let malformed_source = page.range().end().get() > extent
            || (page.preceding() == PageEdgeFact::DocumentBoundary)
                != (page.range().start().get() == 0)
            || (page.following() == PageEdgeFact::DocumentBoundary)
                != (page.range().end().get() == extent)
            || page.end_of_source() != (page.range().end().get() == extent);
        if malformed_source {
            let key = active.key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
        }
        let selection_end = active.key.selection().end();
        if page.range().start() != active.next_offset || page.range().end() <= active.next_offset {
            let key = active.key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
        }
        let consumed_end = page.range().end().min(selection_end);
        let consumed_len = usize::try_from(consumed_end.get() - page.range().start().get())
            .map_err(|_| ClipboardError::ObsoletePage(actual))?;
        let Some(consumed_text) = page.text().get(..consumed_len) else {
            let key = active.key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
        };
        let page_line_breaks = consumed_text.bytes().filter(|byte| *byte == b'\n').count() as u64;
        let Some(source_line_breaks) = active.source_line_breaks.checked_add(page_line_breaks)
        else {
            let key = active.key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
        };
        self.active
            .as_mut()
            .expect("active page checked")
            .source_line_breaks = source_line_breaks;
        if let Some(outcome) = self.collect_page(&page) {
            let key = self.active.as_ref().expect("active").key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(outcome));
        }
        let active = self.active.as_mut().expect("active");
        active.pending = None;
        active.next_offset = consumed_end;
        if active.next_offset == active.key.selection().end() {
            if active.open_atom.is_some() {
                let key = active.key;
                self.finish(key);
                return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
            }
            Ok(self.complete_collection())
        } else {
            active.state = ClipboardState::Collecting;
            Ok(ClipboardProgress::NeedPage {
                key: active.key,
                next_offset: active.next_offset,
            })
        }
    }

    pub(super) fn collect_page(&mut self, page: &RangePage) -> Option<ClipboardCompletion> {
        let limit = self.limits.max_bytes;
        let active = self
            .active
            .as_mut()
            .expect("page admission requires active collection");
        let page_start = page.range().start().get();
        let consumed_end = page.range().end().min(active.key.selection().end());
        let Ok(consumed_len) = usize::try_from(consumed_end.get() - page_start) else {
            return Some(ClipboardCompletion::Malformed);
        };
        let Some(consumed_text) = page.text().get(..consumed_len) else {
            return Some(ClipboardCompletion::Malformed);
        };
        let mut cursor = 0usize;

        for atom in page
            .atoms()
            .iter()
            .take_while(|atom| atom.fragment_range().start() < consumed_end)
        {
            if !active.key.selection().contains(atom.global_range()) {
                return Some(ClipboardCompletion::Malformed);
            }
            let Ok(fragment_start) =
                usize::try_from(atom.fragment_range().start().get() - page_start)
            else {
                return Some(ClipboardCompletion::Malformed);
            };
            let Ok(fragment_end) = usize::try_from(atom.fragment_range().end().get() - page_start)
            else {
                return Some(ClipboardCompletion::Malformed);
            };
            if fragment_start < cursor
                || !page.text().is_char_boundary(fragment_start)
                || !page.text().is_char_boundary(fragment_end)
            {
                return Some(ClipboardCompletion::Malformed);
            }
            if Self::append(
                &mut active.output,
                &consumed_text[cursor..fragment_start],
                limit,
            )
            .is_err()
            {
                return Some(ClipboardCompletion::TooLarge);
            }

            if atom.global_range().start() < page.range().start() {
                let Some(open) = &active.open_atom else {
                    return Some(ClipboardCompletion::Malformed);
                };
                if open.id != atom.id()
                    || open.global_range != atom.global_range()
                    || active.output.get(open.fallback_output.clone()) != Some(atom.fallback_copy())
                {
                    return Some(ClipboardCompletion::Malformed);
                }
            } else {
                if active.open_atom.is_some() {
                    return Some(ClipboardCompletion::Malformed);
                }
                let fallback_start = active.output.len();
                if Self::append(&mut active.output, atom.fallback_copy(), limit).is_err() {
                    return Some(ClipboardCompletion::TooLarge);
                }
                let fallback_end = active.output.len();
                if atom.global_range().end() > page.range().end() {
                    active.open_atom = Some(OpenAtom {
                        id: atom.id(),
                        global_range: atom.global_range(),
                        fallback_output: fallback_start..fallback_end,
                    });
                }
            }
            if atom.global_range().end() <= page.range().end() {
                active.open_atom = None;
            }
            cursor = fragment_end;
        }

        if active.open_atom.is_some() && page.atoms().is_empty() {
            return Some(ClipboardCompletion::Malformed);
        }
        if Self::append(&mut active.output, &consumed_text[cursor..], limit).is_err() {
            return Some(ClipboardCompletion::TooLarge);
        }
        None
    }

    fn append(output: &mut String, text: &str, limit: usize) -> Result<(), ()> {
        let next = output.len().checked_add(text.len()).ok_or(())?;
        if next > limit {
            return Err(());
        }
        output.push_str(text);
        Ok(())
    }

    pub(super) fn complete_collection(&mut self) -> ClipboardProgress {
        let active = self.active.as_mut().expect("collection is active");
        let text = std::mem::take(&mut active.output);
        active.pending = None;
        active.state = ClipboardState::AwaitingWrite;
        ClipboardProgress::Write(ClipboardWriteRequest {
            key: active.key,
            text,
        })
    }
}
