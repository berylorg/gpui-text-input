use std::cmp::Ordering;

use super::*;

impl RangeClipboardCoordinator {
    fn prepared_step(
        &self,
        kind: PreparedClipboardStepKind,
        peak: ClipboardOwnershipCharge,
        successor: ClipboardOwnershipCharge,
    ) -> ClipboardPreparedStep {
        let active = self
            .active
            .as_ref()
            .expect("prepared clipboard step is active");
        ClipboardPreparedStep {
            coordinator_instance: self.coordinator_instance,
            operation_key: active.key,
            operation_identity: active.operation_identity,
            generation: self.preparation_generation,
            peak,
            successor,
            kind,
        }
    }

    fn unchanged_step(&self, kind: PreparedClipboardStepKind) -> ClipboardPreparedStep {
        let charge = self.ownership_charge();
        let successor = if matches!(
            kind,
            PreparedClipboardStepKind::Terminal(_)
                | PreparedClipboardStepKind::TerminalTextResponse { .. }
                | PreparedClipboardStepKind::TerminalObjectResponse { .. }
        ) {
            ClipboardOwnershipCharge::default()
        } else {
            charge
        };
        self.prepared_step(kind, charge, successor)
    }

    pub fn prepare_text_page(
        &self,
        page: &RangePage,
    ) -> Result<ClipboardPreparedStep, ClipboardError> {
        self.ensure_preparation_generation_available()?;
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
        if active.retained_text_response.is_some() {
            return Err(ClipboardError::PreparationInUse);
        }
        let retained_too_large = u64::try_from(page.retained_bytes())
            .map_or(true, |bytes| bytes > self.limits.max_text_page_bytes);
        if retained_too_large || self.text_page_is_malformed(page) {
            return Ok(
                self.unchanged_step(PreparedClipboardStepKind::TerminalTextResponse {
                    key: actual,
                    response_identity: page.response_allocation_identity(),
                    response_charge: page.retained_charge(),
                    completion: if retained_too_large {
                        ClipboardCompletion::TextPageTooLarge
                    } else {
                        ClipboardCompletion::Malformed
                    },
                }),
            );
        }
        let target = active.text_target.expect("text page requires a target");
        let consumed_end = page.range().end().min(target);
        let consumed_len = usize::try_from(consumed_end.get() - page.range().start().get())
            .map_err(|_| ClipboardError::ObsoletePage(actual))?;
        let Some(consumed_text) = page.text().get(..consumed_len) else {
            return Ok(
                self.unchanged_step(PreparedClipboardStepKind::TerminalTextResponse {
                    key: actual,
                    response_identity: page.response_allocation_identity(),
                    response_charge: page.retained_charge(),
                    completion: ClipboardCompletion::Malformed,
                }),
            );
        };
        let line_breaks = if active.phase == ClipboardCollectionPhase::Collecting {
            u64::try_from(consumed_text.bytes().filter(|byte| *byte == b'\n').count())
                .map_err(|_| ClipboardError::PreparationOverflow)?
        } else {
            0
        };
        active
            .source_line_breaks
            .checked_add(line_breaks)
            .ok_or(ClipboardError::PreparationOverflow)?;
        let charge = page.retained_charge();
        let payload_bytes = charge
            .bytes()
            .checked_sub(std::mem::size_of::<RangePage>())
            .ok_or(ClipboardError::PreparationOverflow)?;
        let payload_items = charge.items().checked_sub(1).unwrap_or(0);
        let current = self.ownership_charge();
        let successor = ClipboardOwnershipCharge {
            bytes: current
                .bytes
                .checked_add(payload_bytes)
                .ok_or(ClipboardError::PreparationOverflow)?,
            items: current
                .items
                .checked_add(payload_items)
                .ok_or(ClipboardError::PreparationOverflow)?,
        };
        Ok(self.prepared_step(
            PreparedClipboardStepKind::RetainTextResponse {
                key: actual,
                response_identity: page.response_allocation_identity(),
                response_charge: page.retained_charge(),
                consumed_end,
                consumed_len,
                line_breaks,
            },
            successor,
            successor,
        ))
    }

    pub fn commit_text_page(
        &mut self,
        page: RangePage,
        step: ClipboardPreparedStep,
    ) -> Result<ClipboardPreparedCommit, ClipboardError> {
        let next_generation = self.check_prepared(&step)?;
        let (key, response_identity, response_charge, consumed_end, consumed_len, line_breaks) =
            match step.kind {
                PreparedClipboardStepKind::RetainTextResponse {
                    key,
                    response_identity,
                    response_charge,
                    consumed_end,
                    consumed_len,
                    line_breaks,
                } => (
                    key,
                    response_identity,
                    response_charge,
                    consumed_end,
                    consumed_len,
                    line_breaks,
                ),
                PreparedClipboardStepKind::TerminalTextResponse {
                    key,
                    response_identity,
                    response_charge,
                    completion,
                } => {
                    if page.key() != key
                        || page.response_allocation_identity() != response_identity
                        || page.retained_charge() != response_charge
                    {
                        return Err(ClipboardError::WrongPreparation);
                    }
                    let active = self.active.as_ref().ok_or(ClipboardError::NoActive)?;
                    if active.pending_text != Some(key) || active.retained_text_response.is_some() {
                        return Err(ClipboardError::StalePreparation);
                    }
                    let clipboard_key = active.key;
                    self.finish(clipboard_key);
                    self.finish_prepared(next_generation, step.successor);
                    return Ok(ClipboardPreparedCommit {
                        progress: Some(ClipboardProgress::Terminal(completion)),
                        released_text_page: Some(key),
                        released_object_page: None,
                    });
                }
                _ => return Err(ClipboardError::WrongPreparation),
            };
        if page.key() != key {
            return Err(ClipboardError::WrongPageKey {
                expected: key,
                actual: page.key(),
            });
        }
        if page.response_allocation_identity() != response_identity
            || page.retained_charge() != response_charge
        {
            return Err(ClipboardError::WrongPreparation);
        }
        let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
        if active.pending_text != Some(key) || active.retained_text_response.is_some() {
            return Err(ClipboardError::StalePreparation);
        }
        active.source_line_breaks = active
            .source_line_breaks
            .checked_add(line_breaks)
            .ok_or(ClipboardError::PreparationOverflow)?;
        active.retained_text_response = Some(RetainedTextResponse {
            page,
            consumed_end,
            consumed_len,
            cursor: 0,
            atom_index: 0,
        });
        self.finish_prepared(next_generation, step.successor);
        Ok(ClipboardPreparedCommit::empty())
    }

    pub fn prepare_object_page(
        &self,
        page: &ObjectPage,
    ) -> Result<ClipboardPreparedStep, ClipboardError> {
        self.ensure_preparation_generation_available()?;
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
        if active.retained_object_response.is_some() || active.retained_text_response.is_some() {
            return Err(ClipboardError::PreparationInUse);
        }
        if page.retained_charge().bytes() > self.limits.max_object_page_retained_bytes()
            || page.objects().len() > self.limits.max_object_page_objects()
        {
            return Ok(
                self.unchanged_step(PreparedClipboardStepKind::TerminalObjectResponse {
                    key: actual,
                    response_identity: page.response_allocation_identity(),
                    response_charge: page.retained_charge(),
                    completion: ClipboardCompletion::Malformed,
                }),
            );
        }
        let (payload_bytes, payload_items) = page
            .clipboard_allocation_charge()
            .ok_or(ClipboardError::PreparationOverflow)?;
        let current = self.ownership_charge();
        let successor = ClipboardOwnershipCharge {
            bytes: current
                .bytes
                .checked_add(payload_bytes)
                .ok_or(ClipboardError::PreparationOverflow)?,
            items: current
                .items
                .checked_add(payload_items)
                .ok_or(ClipboardError::PreparationOverflow)?,
        };
        Ok(self.prepared_step(
            PreparedClipboardStepKind::RetainObjectResponse {
                key: actual,
                response_identity: page.response_allocation_identity(),
                response_charge: page.retained_charge(),
            },
            successor,
            successor,
        ))
    }

    pub fn commit_object_page(
        &mut self,
        page: ObjectPage,
        step: ClipboardPreparedStep,
    ) -> Result<ClipboardPreparedCommit, ClipboardError> {
        let next_generation = self.check_prepared(&step)?;
        let (key, response_identity, response_charge) = match step.kind {
            PreparedClipboardStepKind::RetainObjectResponse {
                key,
                response_identity,
                response_charge,
            } => (key, response_identity, response_charge),
            PreparedClipboardStepKind::TerminalObjectResponse {
                key,
                response_identity,
                response_charge,
                completion,
            } => {
                if page.key() != key
                    || page.response_allocation_identity() != response_identity
                    || page.retained_charge() != response_charge
                {
                    return Err(ClipboardError::WrongPreparation);
                }
                let active = self.active.as_ref().ok_or(ClipboardError::NoActive)?;
                if active.pending_object != Some(key)
                    || active.retained_object_response.is_some()
                    || active.retained_text_response.is_some()
                {
                    return Err(ClipboardError::StalePreparation);
                }
                let clipboard_key = active.key;
                self.finish(clipboard_key);
                self.finish_prepared(next_generation, step.successor);
                return Ok(ClipboardPreparedCommit {
                    progress: Some(ClipboardProgress::Terminal(completion)),
                    released_text_page: None,
                    released_object_page: Some(key),
                });
            }
            _ => return Err(ClipboardError::WrongPreparation),
        };
        if page.key() != key {
            return Err(ClipboardError::WrongObjectPageKey {
                expected: key,
                actual: page.key(),
            });
        }
        if page.response_allocation_identity() != response_identity
            || page.retained_charge() != response_charge
        {
            return Err(ClipboardError::WrongPreparation);
        }
        let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
        if active.pending_object != Some(key) || active.retained_object_response.is_some() {
            return Err(ClipboardError::StalePreparation);
        }
        let (key, objects, complete, continuation) = page.into_clipboard_parts();
        active.retained_object_response = Some(RetainedObjectResponse {
            key,
            objects: objects.into(),
            complete,
            continuation,
        });
        self.finish_prepared(next_generation, step.successor);
        Ok(ClipboardPreparedCommit::empty())
    }

    pub fn prepare_next(&self) -> Result<ClipboardPreparedStep, ClipboardError> {
        self.ensure_preparation_generation_available()?;
        let active = self.active.as_ref().ok_or(ClipboardError::NoActive)?;
        if matches!(
            active.state,
            ClipboardState::AwaitingProvenancePage | ClipboardState::AwaitingWrite
        ) {
            return Err(ClipboardError::WrongState {
                expected: ClipboardState::CollectingObjects,
                actual: active.state,
            });
        }
        if active
            .provenance
            .as_ref()
            .is_some_and(|provenance| provenance.builder_is_full())
        {
            return self.prepare_emit_provenance();
        }
        if active.retained_text_response.is_some() {
            return self.prepare_text_response_step();
        }
        self.prepare_merge_step()
    }

    pub fn commit_prepared(
        &mut self,
        step: ClipboardPreparedStep,
    ) -> Result<ClipboardPreparedCommit, ClipboardError> {
        let next_generation = self.check_prepared(&step)?;
        let successor = step.successor;
        let mut result = ClipboardPreparedCommit::empty();
        match step.kind {
            PreparedClipboardStepKind::AllocateOutput => {
                let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
                if active.output.capacity() != 0 || self.limits.max_bytes == 0 {
                    return Err(ClipboardError::StalePreparation);
                }
                if active.output.allocate(self.limits.max_bytes).is_err() {
                    return Ok(self.terminal_local_failure(
                        next_generation,
                        ClipboardCompletion::AllocationFailed,
                    ));
                }
            }
            PreparedClipboardStepKind::AllocateProvenanceBuilder => {
                let allocation = self
                    .active
                    .as_mut()
                    .and_then(|active| active.provenance.as_mut())
                    .ok_or(ClipboardError::StalePreparation)?
                    .allocate_builder();
                if allocation.is_err() {
                    return Ok(self.terminal_local_failure(
                        next_generation,
                        ClipboardCompletion::AllocationFailed,
                    ));
                }
            }
            PreparedClipboardStepKind::AppendText { start, end } => {
                let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
                let response = active
                    .retained_text_response
                    .as_mut()
                    .ok_or(ClipboardError::StalePreparation)?;
                let text = response
                    .page
                    .text()
                    .get(start..end)
                    .ok_or(ClipboardError::StalePreparation)?;
                active
                    .output
                    .push_str(text)
                    .expect("prepared output append fits exact backing");
                response.cursor = end;
            }
            PreparedClipboardStepKind::AppendAtom {
                atom_index,
                fragment_end,
                opens,
            } => {
                let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
                let response = active
                    .retained_text_response
                    .as_mut()
                    .ok_or(ClipboardError::StalePreparation)?;
                let atom = response
                    .page
                    .atoms()
                    .get(atom_index)
                    .ok_or(ClipboardError::StalePreparation)?;
                let fallback_start = active.output.len();
                active
                    .output
                    .push_str(atom.fallback_copy())
                    .expect("prepared atom append fits exact backing");
                active.open_atom = opens.then(|| OpenAtom {
                    id: atom.id(),
                    global_range: atom.global_range(),
                    fallback_output: fallback_start..active.output.len(),
                });
                response.cursor = fragment_end;
                response.atom_index += 1;
            }
            PreparedClipboardStepKind::AdvanceAtom {
                atom_index,
                fragment_end,
                closes,
            } => {
                let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
                let response = active
                    .retained_text_response
                    .as_mut()
                    .ok_or(ClipboardError::StalePreparation)?;
                if response.atom_index != atom_index {
                    return Err(ClipboardError::StalePreparation);
                }
                response.cursor = fragment_end;
                response.atom_index += 1;
                if closes {
                    active.open_atom = None;
                }
            }
            PreparedClipboardStepKind::FinishTextResponse => {
                let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
                let response = active
                    .retained_text_response
                    .take()
                    .ok_or(ClipboardError::StalePreparation)?;
                active.pending_text = None;
                active.text_cursor = response.consumed_end;
                result.released_text_page = Some(response.page.key());
                let target = active
                    .text_target
                    .expect("retained text response has target");
                if active.text_cursor == target {
                    active.text_target = None;
                    active.state = ClipboardState::CollectingObjects;
                } else {
                    active.state = ClipboardState::CollectingText;
                    result.progress = Some(ClipboardProgress::NeedTextPage {
                        key: active.key,
                        next_offset: active.text_cursor,
                        target,
                    });
                }
            }
            PreparedClipboardStepKind::TakeObject => {
                let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
                let object = active
                    .retained_object_response
                    .as_mut()
                    .and_then(|response| response.objects.pop_front())
                    .ok_or(ClipboardError::StalePreparation)?;
                active.current_object = Some(object);
            }
            PreparedClipboardStepKind::ProcessObject {
                selected,
                leading,
                trailing,
            } => self.commit_object_step(selected, leading, trailing)?,
            PreparedClipboardStepKind::FinishObjectResponse => {
                let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
                let response = active
                    .retained_object_response
                    .take()
                    .ok_or(ClipboardError::StalePreparation)?;
                if !response.objects.is_empty() {
                    return Err(ClipboardError::StalePreparation);
                }
                active.pending_object = None;
                active.object_cursor = response.continuation;
                active.object_page_complete = response.complete;
                active.state = ClipboardState::CollectingObjects;
                result.released_object_page = Some(response.key);
            }
            PreparedClipboardStepKind::EmitProvenance => {
                let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
                let page = active
                    .provenance
                    .as_mut()
                    .ok_or(ClipboardError::StalePreparation)?
                    .emit(active.key);
                let Ok(page) = page else {
                    return Ok(self
                        .terminal_local_failure(next_generation, ClipboardCompletion::Malformed));
                };
                active.state = ClipboardState::AwaitingProvenancePage;
                result.progress = Some(ClipboardProgress::ProvenancePage(page));
            }
            PreparedClipboardStepKind::CompleteCollection => {
                result.progress = Some(self.complete_collection());
            }
            PreparedClipboardStepKind::NeedTextPage { target } => {
                let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
                active.text_target = Some(target);
                active.state = ClipboardState::CollectingText;
                result.progress = Some(ClipboardProgress::NeedTextPage {
                    key: active.key,
                    next_offset: active.text_cursor,
                    target,
                });
            }
            PreparedClipboardStepKind::NeedObjectPage => {
                let active = self.active.as_ref().ok_or(ClipboardError::NoActive)?;
                result.progress = Some(ClipboardProgress::NeedObjectPage {
                    key: active.key,
                    cursor: active.object_cursor,
                });
            }
            PreparedClipboardStepKind::Terminal(outcome) => {
                let key = self.active.as_ref().ok_or(ClipboardError::NoActive)?.key;
                self.finish(key);
                result.progress = Some(ClipboardProgress::Terminal(outcome));
            }
            PreparedClipboardStepKind::RetainTextResponse { .. }
            | PreparedClipboardStepKind::RetainObjectResponse { .. }
            | PreparedClipboardStepKind::TerminalTextResponse { .. }
            | PreparedClipboardStepKind::TerminalObjectResponse { .. } => {
                return Err(ClipboardError::WrongPreparation);
            }
        }
        self.finish_prepared(next_generation, successor);
        Ok(result)
    }

    fn check_prepared(&self, step: &ClipboardPreparedStep) -> Result<u64, ClipboardError> {
        if self.coordinator_instance != step.coordinator_instance {
            return Err(ClipboardError::WrongPreparation);
        }
        let active = self
            .active
            .as_ref()
            .ok_or(ClipboardError::StalePreparation)?;
        if active.key != step.operation_key || active.operation_identity != step.operation_identity
        {
            return Err(ClipboardError::StalePreparation);
        }
        if step.generation != self.preparation_generation {
            return Err(ClipboardError::StalePreparation);
        }
        self.preparation_generation
            .checked_add(1)
            .ok_or(ClipboardError::PreparationOverflow)
    }

    pub(super) fn ensure_preparation_generation_available(&self) -> Result<(), ClipboardError> {
        self.preparation_generation
            .checked_add(1)
            .map(|_| ())
            .ok_or(ClipboardError::PreparationOverflow)
    }

    fn finish_prepared(&mut self, next_generation: u64, expected: ClipboardOwnershipCharge) {
        let actual = self.ownership_charge();
        debug_assert_eq!(actual, expected);
        self.preparation_generation = next_generation;
    }

    fn terminal_local_failure(
        &mut self,
        next_generation: u64,
        completion: ClipboardCompletion,
    ) -> ClipboardPreparedCommit {
        let active = self
            .active
            .as_ref()
            .expect("prepared clipboard step is active");
        let key = active.key;
        let released_text_page = active
            .retained_text_response
            .as_ref()
            .map(|response| response.page.key());
        let released_object_page = active
            .retained_object_response
            .as_ref()
            .map(|response| response.key);
        self.finish(key);
        self.preparation_generation = next_generation;
        ClipboardPreparedCommit {
            progress: Some(ClipboardProgress::Terminal(completion)),
            released_text_page,
            released_object_page,
        }
    }

    fn prepare_output_capacity(
        &self,
        append_bytes: usize,
    ) -> Result<Option<ClipboardPreparedStep>, ClipboardError> {
        let active = self.active.as_ref().ok_or(ClipboardError::NoActive)?;
        let next_len = active
            .output
            .len()
            .checked_add(append_bytes)
            .ok_or(ClipboardError::PreparationOverflow)?;
        if next_len > self.limits.max_bytes {
            return Ok(Some(self.unchanged_step(
                PreparedClipboardStepKind::Terminal(ClipboardCompletion::TooLarge),
            )));
        }
        if next_len <= active.output.capacity() {
            return Ok(None);
        }
        let current = self.ownership_charge();
        let delta = self.limits.max_bytes;
        let successor = ClipboardOwnershipCharge {
            bytes: current
                .bytes
                .checked_add(delta)
                .ok_or(ClipboardError::PreparationOverflow)?,
            items: current
                .items
                .checked_add(delta)
                .ok_or(ClipboardError::PreparationOverflow)?,
        };
        Ok(Some(self.prepared_step(
            PreparedClipboardStepKind::AllocateOutput,
            successor,
            successor,
        )))
    }

    fn prepare_provenance_builder(&self) -> Result<Option<ClipboardPreparedStep>, ClipboardError> {
        let Some(provenance) = self
            .active
            .as_ref()
            .and_then(|active| active.provenance.as_ref())
        else {
            return Ok(None);
        };
        if provenance.items.is_some() {
            return Ok(None);
        }
        let (bytes, items) = provenance
            .builder_allocation_charge()
            .ok_or(ClipboardError::PreparationOverflow)?;
        let current = self.ownership_charge();
        let successor = ClipboardOwnershipCharge {
            bytes: current
                .bytes
                .checked_add(bytes)
                .ok_or(ClipboardError::PreparationOverflow)?,
            items: current
                .items
                .checked_add(items)
                .ok_or(ClipboardError::PreparationOverflow)?,
        };
        Ok(Some(self.prepared_step(
            PreparedClipboardStepKind::AllocateProvenanceBuilder,
            successor,
            successor,
        )))
    }

    fn prepare_emit_provenance(&self) -> Result<ClipboardPreparedStep, ClipboardError> {
        let active = self.active.as_ref().ok_or(ClipboardError::NoActive)?;
        let provenance = active
            .provenance
            .as_ref()
            .ok_or(ClipboardError::StalePreparation)?;
        let (bytes, items) = provenance
            .emitted_ownership_charge()
            .ok_or(ClipboardError::StalePreparation)?;
        let old = provenance
            .ownership_charge()
            .ok_or(ClipboardError::PreparationOverflow)?;
        let current = self.ownership_charge();
        let successor = ClipboardOwnershipCharge {
            bytes: current
                .bytes
                .checked_sub(old.0)
                .and_then(|value| value.checked_add(bytes))
                .ok_or(ClipboardError::PreparationOverflow)?,
            items: current
                .items
                .checked_sub(old.1)
                .and_then(|value| value.checked_add(items))
                .ok_or(ClipboardError::PreparationOverflow)?,
        };
        Ok(self.prepared_step(
            PreparedClipboardStepKind::EmitProvenance,
            successor,
            successor,
        ))
    }

    fn prepare_text_response_step(&self) -> Result<ClipboardPreparedStep, ClipboardError> {
        let active = self.active.as_ref().ok_or(ClipboardError::NoActive)?;
        let response = active
            .retained_text_response
            .as_ref()
            .ok_or(ClipboardError::StalePreparation)?;
        if active.phase == ClipboardCollectionPhase::Classifying {
            if response
                .page
                .atoms()
                .iter()
                .any(|atom| atom.fragment_range().start() < response.consumed_end)
            {
                return Ok(self.unchanged_step(PreparedClipboardStepKind::Terminal(
                    ClipboardCompletion::Propagate(active.kind),
                )));
            }
            return self.prepare_finish_text_response();
        }
        let next_atom = response
            .page
            .atoms()
            .get(response.atom_index)
            .filter(|atom| atom.fragment_range().start() < response.consumed_end);
        if let Some(atom) = next_atom {
            let page_start = response.page.range().start().get();
            let fragment_start = usize::try_from(atom.fragment_range().start().get() - page_start)
                .map_err(|_| ClipboardError::PreparationOverflow)?;
            let fragment_end = usize::try_from(atom.fragment_range().end().get() - page_start)
                .map_err(|_| ClipboardError::PreparationOverflow)?;
            let selection = ByteRange::new(
                active.key.selection().start().byte_offset,
                active.key.selection().end().byte_offset,
            )
            .expect("selection bytes ordered");
            if fragment_start < response.cursor
                || fragment_end > response.consumed_len
                || !response.page.text().is_char_boundary(fragment_start)
                || !response.page.text().is_char_boundary(fragment_end)
                || !selection.contains(atom.global_range())
            {
                return Ok(self.unchanged_step(PreparedClipboardStepKind::Terminal(
                    ClipboardCompletion::Malformed,
                )));
            }
            if response.cursor < fragment_start {
                if let Some(step) =
                    self.prepare_output_capacity(fragment_start - response.cursor)?
                {
                    return Ok(step);
                }
                return Ok(self.unchanged_step(PreparedClipboardStepKind::AppendText {
                    start: response.cursor,
                    end: fragment_start,
                }));
            }
            if atom.global_range().start() < response.page.range().start() {
                let valid = active.open_atom.as_ref().is_some_and(|open| {
                    open.id == atom.id()
                        && open.global_range == atom.global_range()
                        && active.output.get(open.fallback_output.clone())
                            == Some(atom.fallback_copy())
                });
                if !valid {
                    return Ok(self.unchanged_step(PreparedClipboardStepKind::Terminal(
                        ClipboardCompletion::Malformed,
                    )));
                }
                return Ok(self.unchanged_step(PreparedClipboardStepKind::AdvanceAtom {
                    atom_index: response.atom_index,
                    fragment_end,
                    closes: atom.global_range().end() <= response.page.range().end(),
                }));
            }
            if active.open_atom.is_some() {
                return Ok(self.unchanged_step(PreparedClipboardStepKind::Terminal(
                    ClipboardCompletion::Malformed,
                )));
            }
            if let Some(step) = self.prepare_output_capacity(atom.fallback_copy().len())? {
                return Ok(step);
            }
            return Ok(self.unchanged_step(PreparedClipboardStepKind::AppendAtom {
                atom_index: response.atom_index,
                fragment_end,
                opens: atom.global_range().end() > response.page.range().end(),
            }));
        }
        if active.open_atom.is_some() && response.page.atoms().is_empty() {
            return Ok(self.unchanged_step(PreparedClipboardStepKind::Terminal(
                ClipboardCompletion::Malformed,
            )));
        }
        if response.cursor < response.consumed_len {
            if let Some(step) =
                self.prepare_output_capacity(response.consumed_len - response.cursor)?
            {
                return Ok(step);
            }
            return Ok(self.unchanged_step(PreparedClipboardStepKind::AppendText {
                start: response.cursor,
                end: response.consumed_len,
            }));
        }
        self.prepare_finish_text_response()
    }

    fn prepare_finish_text_response(&self) -> Result<ClipboardPreparedStep, ClipboardError> {
        let response = self
            .active
            .as_ref()
            .and_then(|active| active.retained_text_response.as_ref())
            .ok_or(ClipboardError::StalePreparation)?;
        let current = self.ownership_charge();
        let charge = response.page.retained_charge();
        let successor = ClipboardOwnershipCharge {
            bytes: current
                .bytes
                .checked_sub(
                    charge
                        .bytes()
                        .checked_sub(std::mem::size_of::<RangePage>())
                        .ok_or(ClipboardError::PreparationOverflow)?,
                )
                .ok_or(ClipboardError::PreparationOverflow)?,
            items: current
                .items
                .checked_sub(charge.items().checked_sub(1).unwrap_or(0))
                .ok_or(ClipboardError::PreparationOverflow)?,
        };
        Ok(self.prepared_step(
            PreparedClipboardStepKind::FinishTextResponse,
            current,
            successor,
        ))
    }

    fn prepare_merge_step(&self) -> Result<ClipboardPreparedStep, ClipboardError> {
        let active = self.active.as_ref().ok_or(ClipboardError::NoActive)?;
        if active.current_object.is_none() {
            if active
                .retained_object_response
                .as_ref()
                .is_some_and(|response| !response.objects.is_empty())
            {
                return Ok(self.unchanged_step(PreparedClipboardStepKind::TakeObject));
            }
            if active.retained_object_response.is_some() {
                return self.prepare_finish_object_response();
            }
            if !active.object_page_complete {
                return Ok(self.unchanged_step(PreparedClipboardStepKind::NeedObjectPage));
            }
            let end = active.key.selection().end().byte_offset;
            if active.text_cursor < end {
                return Ok(
                    self.unchanged_step(PreparedClipboardStepKind::NeedTextPage { target: end })
                );
            }
            let selection = active.key.selection();
            let start_proven = active.start_gap_proven
                || (selection.start().gap == crate::InlineObjectGap::NoObjects
                    && !active.start_anchor_had_object);
            let end_proven = active.end_gap_proven
                || (selection.end().gap == crate::InlineObjectGap::NoObjects
                    && !active.end_anchor_had_object);
            if !start_proven || !end_proven || active.open_atom.is_some() {
                return Ok(self.unchanged_step(PreparedClipboardStepKind::Terminal(
                    ClipboardCompletion::Malformed,
                )));
            }
            if active
                .provenance
                .as_ref()
                .is_some_and(|provenance| provenance.has_items())
            {
                return self.prepare_emit_provenance();
            }
            return if active.phase == ClipboardCollectionPhase::Classifying {
                Ok(self.unchanged_step(PreparedClipboardStepKind::CompleteCollection))
            } else {
                let current = self.ownership_charge();
                Ok(self.prepared_step(
                    PreparedClipboardStepKind::CompleteCollection,
                    current,
                    ClipboardOwnershipCharge {
                        bytes: std::mem::size_of::<ActiveClipboard>(),
                        items: 1,
                    },
                ))
            };
        }
        if active
            .retained_object_response
            .as_ref()
            .is_some_and(|response| response.objects.is_empty())
        {
            return self.prepare_finish_object_response();
        }
        let current_object = active.current_object.as_ref().unwrap();
        let next_cursor = active
            .retained_object_response
            .as_ref()
            .and_then(|response| response.objects.front())
            .map(InlineObjectFact::cursor);
        if next_cursor.is_none() && !active.object_page_complete {
            return Ok(self.unchanged_step(PreparedClipboardStepKind::NeedObjectPage));
        }
        let anchor = current_object.anchor();
        if active.text_cursor < anchor {
            return Ok(
                self.unchanged_step(PreparedClipboardStepKind::NeedTextPage { target: anchor })
            );
        }
        if active.text_cursor > anchor {
            return Ok(self.unchanged_step(PreparedClipboardStepKind::Terminal(
                ClipboardCompletion::Malformed,
            )));
        }
        let leading = leading_position(current_object, active.prior_object);
        let trailing = trailing_position(current_object, next_cursor);
        let selection = active.key.selection();
        let selected = selection
            .start()
            .compare_in_revision(leading)
            .is_some_and(|order| order != Ordering::Greater)
            && trailing
                .compare_in_revision(selection.end())
                .is_some_and(|order| order != Ordering::Greater);
        if selected && active.phase == ClipboardCollectionPhase::Classifying {
            return Ok(self.unchanged_step(PreparedClipboardStepKind::Terminal(
                ClipboardCompletion::Propagate(active.kind),
            )));
        }
        if selected {
            if let Some(step) =
                self.prepare_output_capacity(current_object.fallback_copy().len())?
            {
                return Ok(step);
            }
            if let Some(step) = self.prepare_provenance_builder()? {
                return Ok(step);
            }
        }
        let current = self.ownership_charge();
        let released_bytes = current_object
            .owned_payload_allocation_bytes()
            .ok_or(ClipboardError::PreparationOverflow)?;
        let successor = ClipboardOwnershipCharge {
            bytes: current
                .bytes
                .checked_sub(released_bytes)
                .ok_or(ClipboardError::PreparationOverflow)?,
            items: current.items,
        };
        Ok(self.prepared_step(
            PreparedClipboardStepKind::ProcessObject {
                selected,
                leading,
                trailing,
            },
            current,
            successor,
        ))
    }

    fn prepare_finish_object_response(&self) -> Result<ClipboardPreparedStep, ClipboardError> {
        let active = self.active.as_ref().ok_or(ClipboardError::NoActive)?;
        let response = active
            .retained_object_response
            .as_ref()
            .ok_or(ClipboardError::StalePreparation)?;
        if !response.objects.is_empty() {
            return Err(ClipboardError::StalePreparation);
        }
        let current = self.ownership_charge();
        let record_bytes = response
            .objects
            .capacity()
            .checked_mul(std::mem::size_of::<InlineObjectFact>())
            .ok_or(ClipboardError::PreparationOverflow)?;
        let successor = ClipboardOwnershipCharge {
            bytes: current
                .bytes
                .checked_sub(record_bytes)
                .ok_or(ClipboardError::PreparationOverflow)?,
            items: current
                .items
                .checked_sub(response.objects.capacity())
                .ok_or(ClipboardError::PreparationOverflow)?,
        };
        Ok(self.prepared_step(
            PreparedClipboardStepKind::FinishObjectResponse,
            current,
            successor,
        ))
    }

    fn commit_object_step(
        &mut self,
        selected: bool,
        leading: SourcePosition,
        trailing: SourcePosition,
    ) -> Result<(), ClipboardError> {
        let active = self.active.as_mut().ok_or(ClipboardError::NoActive)?;
        let current = active
            .current_object
            .take()
            .ok_or(ClipboardError::StalePreparation)?;
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
        if selected {
            let output_start = active.output.len();
            active
                .output
                .push_str(current.fallback_copy())
                .expect("prepared object append fits exact backing");
            let output_end = active.output.len();
            if let Some(provenance) = active.provenance.as_mut() {
                provenance
                    .push(&current, output_start, output_end)
                    .map_err(|_| ClipboardError::StalePreparation)?;
            }
        }
        active.prior_object = Some(current.cursor());
        Ok(())
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
        let provenance = match active.provenance.take() {
            Some(provenance) => match provenance.closure(active.key, active.output.as_str()) {
                Ok(closure) => Some(closure),
                Err(()) => {
                    let key = active.key;
                    self.finish(key);
                    return ClipboardProgress::Terminal(ClipboardCompletion::Malformed);
                }
            },
            None => None,
        };
        let text = std::mem::take(&mut active.output).into_string();
        active.pending_text = None;
        active.pending_object = None;
        active.retained_text_response = None;
        active.retained_object_response = None;
        active.current_object = None;
        active.state = ClipboardState::AwaitingWrite;
        ClipboardProgress::Write(ClipboardWriteRequest {
            key: active.key,
            text,
            provenance,
        })
    }
}

impl ClipboardPreparedCommit {
    fn empty() -> Self {
        Self {
            progress: None,
            released_text_page: None,
            released_object_page: None,
        }
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
