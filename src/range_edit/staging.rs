use super::*;

impl RangeEditCoordinator {
    /// Admits one exact next fragment while retaining no joined source value.
    pub fn stage(&mut self, fragment: MutationFragment) -> Result<(), MutationError> {
        let limits = self.limits;
        let active = self.active_mut(fragment.key(), MutationState::Staging)?;
        if active.terminal_seen {
            return Err(MutationError::PostTerminalFragment);
        }
        if fragment.ordinal() != active.next_ordinal {
            return Err(MutationError::FragmentOutOfOrder {
                expected: active.next_ordinal,
                actual: fragment.ordinal(),
            });
        }
        if active.fragment_count == limits.max_fragments {
            return Err(MutationError::FragmentLimitExceeded);
        }

        let mut next_inserted_bytes = active.inserted_bytes;
        let mut next_inserted_line_breaks = active.inserted_line_breaks;
        let mut terminal_seen = false;
        let added = match fragment.payload() {
            MutationFragmentPayload::Utf8 {
                inserted_offset,
                text,
            } => {
                if *inserted_offset != active.inserted_bytes {
                    return Err(MutationError::InsertOffsetMismatch {
                        expected: active.inserted_bytes,
                        actual: *inserted_offset,
                    });
                }
                next_inserted_bytes = active
                    .inserted_bytes
                    .checked_add(text.len() as u64)
                    .ok_or(MutationError::StagedByteLimitExceeded)?;
                next_inserted_line_breaks = active
                    .inserted_line_breaks
                    .checked_add(text.bytes().filter(|byte| *byte == b'\n').count() as u64)
                    .ok_or(MutationError::StagedByteLimitExceeded)?;
                text.len()
            }
            MutationFragmentPayload::Atom(AtomChange::Insert {
                id,
                inserted_range,
                fallback_copy,
            }) => {
                if inserted_range.is_empty() || inserted_range.end().get() > active.inserted_bytes {
                    return Err(MutationError::MalformedAtomChange);
                }
                let boundary = |offset: u64| {
                    active
                        .fragments
                        .iter()
                        .any(|fragment| match fragment.payload() {
                            MutationFragmentPayload::Utf8 {
                                inserted_offset,
                                text,
                            } => {
                                let Some(local) = offset
                                    .checked_sub(*inserted_offset)
                                    .and_then(|value| usize::try_from(value).ok())
                                else {
                                    return false;
                                };
                                local <= text.len() && text.is_char_boundary(local)
                            }
                            _ => false,
                        })
                };
                if !boundary(inserted_range.start().get()) || !boundary(inserted_range.end().get())
                {
                    return Err(MutationError::MalformedAtomChange);
                }
                let mut previous = None;
                for prior in &active.fragments {
                    if let MutationFragmentPayload::Atom(AtomChange::Insert {
                        id: prior_id,
                        inserted_range: prior_range,
                        ..
                    }) = prior.payload()
                    {
                        if prior_id == id {
                            return Err(MutationError::DuplicateAtomInsert(*id));
                        }
                        previous = Some(*prior_range);
                    }
                }
                if let Some(previous) = previous {
                    if inserted_range.start() < previous.end() {
                        return Err(MutationError::InsertedAtomRangeOutOfOrder {
                            previous,
                            actual: *inserted_range,
                        });
                    }
                }
                fallback_copy.len()
            }
            MutationFragmentPayload::Atom(AtomChange::Remove { id, source_range }) => {
                if source_range.is_empty() || !active.proposal.replacement().contains(*source_range)
                {
                    return Err(MutationError::MalformedAtomChange);
                }
                let mut previous = None;
                for prior in &active.fragments {
                    if let MutationFragmentPayload::Atom(AtomChange::Remove {
                        id: prior_id,
                        source_range: prior_range,
                    }) = prior.payload()
                    {
                        if prior_id == id {
                            return Err(MutationError::DuplicateAtomRemove(*id));
                        }
                        if prior_range == source_range {
                            return Err(MutationError::DuplicateAtomRemoveRange(*source_range));
                        }
                        previous = Some(*prior_range);
                    }
                }
                if let Some(previous) = previous {
                    if source_range.start() < previous.end() {
                        return Err(MutationError::RemovedAtomRangeOutOfOrder {
                            previous,
                            actual: *source_range,
                        });
                    }
                }
                0
            }
            MutationFragmentPayload::Terminal => {
                terminal_seen = true;
                0
            }
        };
        let staged_bytes = active
            .staged_bytes
            .checked_add(added)
            .filter(|bytes| *bytes <= limits.max_staged_bytes)
            .ok_or(MutationError::StagedByteLimitExceeded)?;
        active.inserted_bytes = next_inserted_bytes;
        active.inserted_line_breaks = next_inserted_line_breaks;
        active.terminal_seen = terminal_seen;
        active.staged_bytes = staged_bytes;
        active.fragment_count += 1;
        active.next_ordinal += 1;
        active.fragments.push(fragment);
        Ok(())
    }
}
