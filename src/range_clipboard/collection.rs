use std::cmp::Ordering;

use super::*;

impl RangeClipboardCoordinator {
    /// Admits one exact bounded object page and resumes the merge join.
    pub fn admit_object_page(
        &mut self,
        page: ObjectPage,
    ) -> Result<ClipboardProgress, ClipboardError> {
        let actual = page.key();
        let Some(active) = &self.active else {
            return Err(ClipboardError::ObsoleteObjectPage(actual));
        };
        let Some(expected) = active.pending_object else {
            return Err(ClipboardError::WrongState {
                expected: ClipboardState::ObjectPagePending,
                actual: active.state,
            });
        };
        if actual != expected {
            return Err(ClipboardError::WrongObjectPageKey { expected, actual });
        }
        if page.retained_charge().bytes() > self.limits.max_object_page_retained_bytes
            || page.objects().len() > self.limits.max_object_page_objects
        {
            let key = active.key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
        }

        let active = self.active.as_mut().expect("active object page checked");
        active.pending_object = None;
        active.object_cursor = page.continuation();
        active.object_page_complete = page.complete();
        active.queued_objects.extend(page.objects().iter().cloned());
        active.state = ClipboardState::CollectingObjects;
        self.advance_merge()
    }

    /// Admits one exact text page and resumes the merge join.
    pub fn admit_text_page(
        &mut self,
        page: RangePage,
    ) -> Result<ClipboardProgress, ClipboardError> {
        let actual = page.key();
        let Some(active) = &self.active else {
            return Err(ClipboardError::ObsoletePage(actual));
        };
        let Some(expected) = active.pending_text else {
            return Err(ClipboardError::WrongState {
                expected: ClipboardState::TextPagePending,
                actual: active.state,
            });
        };
        if actual != expected {
            return Err(ClipboardError::WrongPageKey { expected, actual });
        }
        let retained_too_large = u64::try_from(page.retained_bytes())
            .map_or(true, |bytes| bytes > self.limits.max_text_page_bytes);
        if retained_too_large {
            let key = active.key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(
                ClipboardCompletion::TextPageTooLarge,
            ));
        }
        if self.text_page_is_malformed(&page) {
            let key = active.key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
        }

        let target = active.text_target.expect("text page requires a target");
        let consumed_end = page.range().end().min(target);
        let consumed_len = usize::try_from(consumed_end.get() - page.range().start().get())
            .map_err(|_| ClipboardError::ObsoletePage(actual))?;
        let Some(consumed_text) = page.text().get(..consumed_len) else {
            let key = active.key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
        };
        if active.phase == ClipboardCollectionPhase::Collecting {
            let line_breaks = consumed_text.bytes().filter(|byte| *byte == b'\n').count() as u64;
            let Some(total) = active.source_line_breaks.checked_add(line_breaks) else {
                let key = active.key;
                self.finish(key);
                return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
            };
            self.active.as_mut().expect("active").source_line_breaks = total;
        }
        if let Some(outcome) = self.collect_text_page(&page, consumed_end) {
            let key = self.active.as_ref().expect("active").key;
            self.finish(key);
            return Ok(ClipboardProgress::Terminal(outcome));
        }
        let active = self.active.as_mut().expect("active");
        active.pending_text = None;
        active.text_cursor = consumed_end;
        active.state = ClipboardState::CollectingText;
        if consumed_end == target {
            active.text_target = None;
            active.state = ClipboardState::CollectingObjects;
            self.advance_merge()
        } else {
            Ok(ClipboardProgress::NeedTextPage {
                key: active.key,
                next_offset: active.text_cursor,
                target,
            })
        }
    }

    fn advance_merge(&mut self) -> Result<ClipboardProgress, ClipboardError> {
        loop {
            let active = self
                .active
                .as_mut()
                .expect("merge requires active clipboard");
            if active.current_object.is_none() {
                active.current_object = active.queued_objects.pop_front();
            }
            let Some(current) = active.current_object.as_ref() else {
                if !active.object_page_complete {
                    return Ok(ClipboardProgress::NeedObjectPage {
                        key: active.key,
                        cursor: active.object_cursor,
                    });
                }
                let end = active.key.selection().end().byte_offset;
                if active.text_cursor < end {
                    active.text_target = Some(end);
                    active.state = ClipboardState::CollectingText;
                    return Ok(ClipboardProgress::NeedTextPage {
                        key: active.key,
                        next_offset: active.text_cursor,
                        target: end,
                    });
                }
                let selection = active.key.selection();
                let start_proven = active.start_gap_proven
                    || (selection.start().gap == crate::InlineObjectGap::NoObjects
                        && !active.start_anchor_had_object);
                let end_proven = active.end_gap_proven
                    || (selection.end().gap == crate::InlineObjectGap::NoObjects
                        && !active.end_anchor_had_object);
                if !active.object_page_complete
                    || !start_proven
                    || !end_proven
                    || active.open_atom.is_some()
                {
                    let key = active.key;
                    self.finish(key);
                    return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
                }
                return Ok(self.complete_collection());
            };

            let next_cursor = active.queued_objects.front().map(InlineObjectFact::cursor);
            if next_cursor.is_none() && !active.object_page_complete {
                return Ok(ClipboardProgress::NeedObjectPage {
                    key: active.key,
                    cursor: active.object_cursor,
                });
            }
            let anchor = current.anchor();
            if active.text_cursor < anchor {
                active.text_target = Some(anchor);
                active.state = ClipboardState::CollectingText;
                return Ok(ClipboardProgress::NeedTextPage {
                    key: active.key,
                    next_offset: active.text_cursor,
                    target: anchor,
                });
            }
            if active.text_cursor > anchor {
                let key = active.key;
                self.finish(key);
                return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Malformed));
            }

            let current = active
                .current_object
                .take()
                .expect("current object checked");
            let prior = active.prior_object;
            let next = next_cursor;
            let leading = leading_position(&current, prior);
            let trailing = trailing_position(&current, next);
            let selection = active.key.selection();
            if current.anchor() == selection.start().byte_offset {
                active.start_anchor_had_object = true;
                active.start_gap_proven |=
                    selection.start() == leading || selection.start() == trailing;
            }
            if current.anchor() == selection.end().byte_offset {
                active.end_anchor_had_object = true;
                active.end_gap_proven |= selection.end() == leading || selection.end() == trailing;
            }
            let selected = selection
                .start()
                .compare_in_revision(leading)
                .is_some_and(|order| order != Ordering::Greater)
                && trailing
                    .compare_in_revision(selection.end())
                    .is_some_and(|order| order != Ordering::Greater);
            if selected && active.phase == ClipboardCollectionPhase::Classifying {
                let key = active.key;
                let kind = active.kind;
                self.finish(key);
                return Ok(ClipboardProgress::Terminal(ClipboardCompletion::Propagate(
                    kind,
                )));
            }
            if selected
                && Self::append(
                    &mut active.output,
                    current.fallback_copy(),
                    self.limits.max_bytes,
                )
                .is_err()
            {
                let key = active.key;
                self.finish(key);
                return Ok(ClipboardProgress::Terminal(ClipboardCompletion::TooLarge));
            }
            active.prior_object = Some(current.cursor());
        }
    }

    fn text_page_is_malformed(&self, page: &RangePage) -> bool {
        let active = self.active.as_ref().expect("active");
        let extent = self.binding.extent().byte_len();
        page.range().end().get() > extent
            || page.range().start() != active.text_cursor
            || page.range().end() <= active.text_cursor
            || (page.preceding() == PageEdgeFact::DocumentBoundary)
                != (page.range().start().get() == 0)
            || (page.following() == PageEdgeFact::DocumentBoundary)
                != (page.range().end().get() == extent)
            || page.end_of_source() != (page.range().end().get() == extent)
    }

    fn collect_text_page(
        &mut self,
        page: &RangePage,
        consumed_end: ByteOffset,
    ) -> Option<ClipboardCompletion> {
        let limit = self.limits.max_bytes;
        let active = self.active.as_mut().expect("active text page checked");
        let page_start = page.range().start().get();
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
            let byte_selection = ByteRange::new(
                active.key.selection().start().byte_offset,
                active.key.selection().end().byte_offset,
            )
            .expect("source selection bytes are ordered");
            if !byte_selection.contains(atom.global_range()) {
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
                || fragment_end > consumed_len
                || !page.text().is_char_boundary(fragment_start)
                || !page.text().is_char_boundary(fragment_end)
            {
                return Some(ClipboardCompletion::Malformed);
            }
            if active.phase == ClipboardCollectionPhase::Classifying {
                return Some(ClipboardCompletion::Propagate(active.kind));
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
                if atom.global_range().end() > page.range().end() {
                    active.open_atom = Some(OpenAtom {
                        id: atom.id(),
                        global_range: atom.global_range(),
                        fallback_output: fallback_start..active.output.len(),
                    });
                }
            }
            if atom.global_range().end() <= page.range().end() {
                active.open_atom = None;
            }
            cursor = fragment_end;
        }
        if active.phase == ClipboardCollectionPhase::Classifying {
            return None;
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
        if active.phase == ClipboardCollectionPhase::Classifying {
            active.phase = ClipboardCollectionPhase::Collecting;
            active.text_cursor = active.key.selection().start().byte_offset;
            active.text_target = Some(active.key.selection().end().byte_offset);
            active.output.clear();
            active.open_atom = None;
            active.source_line_breaks = 0;
            active.state = ClipboardState::CollectingText;
            if active.text_cursor != active.text_target.expect("collection target") {
                return ClipboardProgress::NeedTextPage {
                    key: active.key,
                    next_offset: active.text_cursor,
                    target: active.text_target.expect("collection target"),
                };
            }
        }
        let text = std::mem::take(&mut active.output);
        active.pending_text = None;
        active.pending_object = None;
        active.queued_objects.clear();
        active.current_object = None;
        active.state = ClipboardState::AwaitingWrite;
        ClipboardProgress::Write(ClipboardWriteRequest {
            key: active.key,
            text,
        })
    }
}

fn leading_position(object: &InlineObjectFact, prior: Option<ObjectCursor>) -> SourcePosition {
    let neighbor = object.cursor().neighbor();
    let gap = prior
        .filter(|prior| prior.anchor() == object.anchor())
        .map_or_else(
            || crate::InlineObjectGap::before(neighbor),
            |prior| {
                crate::InlineObjectGap::between(prior.neighbor(), neighbor)
                    .expect("strict object-page order creates a valid adjacent gap")
            },
        );
    SourcePosition::new(object.anchor(), gap)
}

fn trailing_position(object: &InlineObjectFact, next: Option<ObjectCursor>) -> SourcePosition {
    let neighbor = object.cursor().neighbor();
    let gap = next
        .filter(|next| next.anchor() == object.anchor())
        .map_or_else(
            || crate::InlineObjectGap::after(neighbor),
            |next| {
                crate::InlineObjectGap::between(neighbor, next.neighbor())
                    .expect("strict object-page order creates a valid adjacent gap")
            },
        );
    SourcePosition::new(object.anchor(), gap)
}
