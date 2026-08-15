use super::*;

impl RangeEditCoordinator {
    /// Admits one exact next fragment while retaining no joined source value.
    pub fn stage(&mut self, fragment: MutationFragment) -> Result<(), MutationError> {
        let key = fragment.key();
        self.active_mut(key, MutationState::Staging)?;
        if let Err(error) = self.stage_validated(fragment) {
            self.finish(key, MutationOutcome::Error, false);
            return Err(error);
        }
        Ok(())
    }

    fn stage_validated(&mut self, fragment: MutationFragment) -> Result<(), MutationError> {
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
        let mut intended = None;
        let (added, added_objects, added_object_bytes, added_presentation_bytes) = match fragment
            .payload()
        {
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
                (text.len(), 0, 0, 0)
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
                (fallback_copy.len(), 0, 0, 0)
            }
            MutationFragmentPayload::Atom(AtomChange::Remove { id, source_range }) => {
                if source_range.is_empty()
                    || !active.proposal.replacement_bytes().contains(*source_range)
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
                (0, 0, 0, 0)
            }
            MutationFragmentPayload::Object(change) => {
                validate_object_change(active, *change)?;
                let (object_bytes, presentation_bytes) = match change {
                    ObjectChange::Insert { object, .. }
                    | ObjectChange::Replace { object, .. }
                    | ObjectChange::Move { object, .. } => {
                        (object.retained_bytes(), object.presentation_bytes())
                    }
                    ObjectChange::Remove { .. } => (0, 0),
                };
                (0, 1, object_bytes, presentation_bytes)
            }
            MutationFragmentPayload::Terminal {
                intended: positions,
            } => {
                terminal_seen = true;
                intended = Some(*positions);
                (0, 0, 0, 0)
            }
        };
        let staged_bytes = active
            .staged_bytes
            .checked_add(added)
            .filter(|bytes| *bytes <= limits.max_staged_bytes)
            .ok_or(MutationError::StagedByteLimitExceeded)?;
        let object_count = active
            .object_count
            .checked_add(added_objects)
            .filter(|count| *count <= limits.max_objects)
            .ok_or(MutationError::ObjectLimitExceeded)?;
        let object_bytes = active
            .object_bytes
            .checked_add(added_object_bytes)
            .filter(|bytes| *bytes <= limits.max_object_bytes)
            .ok_or(MutationError::ObjectByteLimitExceeded)?;
        let presentation_bytes = active
            .presentation_bytes
            .checked_add(added_presentation_bytes)
            .filter(|bytes| *bytes <= limits.max_presentation_bytes)
            .ok_or(MutationError::PresentationByteLimitExceeded)?;
        active.inserted_bytes = next_inserted_bytes;
        active.inserted_line_breaks = next_inserted_line_breaks;
        active.terminal_seen = terminal_seen;
        active.staged_bytes = staged_bytes;
        active.object_count = object_count;
        active.object_bytes = object_bytes;
        active.presentation_bytes = presentation_bytes;
        active.fragment_count += 1;
        active.next_ordinal += 1;
        if intended.is_some() {
            active.intended = intended;
        }
        active.fragments.push(fragment);
        Ok(())
    }
}

fn validate_object_change(
    active: &ActiveMutation,
    change: ObjectChange,
) -> Result<(), MutationError> {
    let replacement = active.proposal.replacement();
    let (target, destination, successor) = match change {
        ObjectChange::Insert { at, object } => (None, Some(at), Some(object)),
        ObjectChange::Remove { target } => (Some(target), None, None),
        ObjectChange::Replace { target, object } => (Some(target), None, Some(object)),
        ObjectChange::Move { target, to, object } => (Some(target), Some(to), Some(object)),
    };
    if target.is_some_and(|target| !source_range_contains(replacement, target.range()))
        || destination
            .is_some_and(|position| !source_range_contains_position(replacement, position))
    {
        return Err(MutationError::ObjectChangeOutsideReplacement);
    }
    if let Some(target) = target
        && successor.is_some_and(|object| {
            matches!(change, ObjectChange::Move { .. }) && object.id() != target.id()
        })
    {
        return Err(MutationError::MalformedObjectChange);
    }
    match change {
        ObjectChange::Insert { at, object } => validate_successor_at(at, object, None)?,
        ObjectChange::Remove { .. } => {}
        ObjectChange::Replace { target, object } => {
            if object.anchor() != target.range().start().byte_offset
                || object.order() != target.order()
            {
                return Err(MutationError::MalformedObjectChange);
            }
            if object.id() != target.id()
                && target_unchanged_neighbors(target)
                    .into_iter()
                    .flatten()
                    .any(|neighbor| neighbor == object.id())
            {
                return Err(MutationError::DuplicateObjectChange(object.id()));
            }
        }
        ObjectChange::Move { target, to, object } => {
            validate_successor_at(to, object, Some(target.id()))?;
        }
    }

    let mut previous_target = None;
    let mut previous_successor = None;
    for prior in &active.fragments {
        let MutationFragmentPayload::Object(prior) = prior.payload() else {
            continue;
        };
        let (prior_target, prior_successor) = match *prior {
            ObjectChange::Insert { object, .. } => (None, Some(object)),
            ObjectChange::Remove { target } => (Some(target), None),
            ObjectChange::Replace { target, object } => (Some(target), Some(object)),
            ObjectChange::Move { target, object, .. } => (Some(target), Some(object)),
        };
        if let Some(actual) = target
            && (prior_target.is_some_and(|prior| prior.id() == actual.id())
                || prior_successor.is_some_and(|prior| prior.id() == actual.id()))
        {
            return Err(MutationError::DuplicateObjectChange(actual.id()));
        }
        if let Some(actual) = successor
            && (prior_target.is_some_and(|prior| prior.id() == actual.id())
                || prior_successor.is_some_and(|prior| prior.id() == actual.id()))
        {
            return Err(MutationError::DuplicateObjectChange(actual.id()));
        }
        if let Some(prior_target) = prior_target {
            previous_target = Some(prior_target);
        }
        if let Some(prior_successor) = prior_successor {
            previous_successor = Some(prior_successor);
        }
    }
    if let (Some(previous), Some(actual)) = (previous_target, target)
        && !matches!(
            positions_cmp(previous.range().end(), actual.range().start()),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        )
    {
        return Err(MutationError::MalformedObjectChange);
    }
    if let (Some(previous), Some(actual)) = (previous_successor, successor) {
        let previous_key = (previous.anchor(), previous.order(), previous.id());
        let actual_key = (actual.anchor(), actual.order(), actual.id());
        if previous.anchor() == actual.anchor() && previous.order() == actual.order() {
            return Err(MutationError::DuplicateSuccessorObjectOrder {
                anchor: actual.anchor(),
                order: actual.order(),
            });
        }
        if previous_key >= actual_key {
            return Err(MutationError::SuccessorObjectsOutOfOrder);
        }
    }
    Ok(())
}

fn target_unchanged_neighbors(target: ObjectTarget) -> [Option<InlineObjectId>; 2] {
    let preceding = match target.range().start().gap {
        InlineObjectGap::Between {
            preceding,
            following,
        } if following.id() == target.id() && following.order() == target.order() => {
            Some(preceding.id())
        }
        InlineObjectGap::Before(following)
            if following.id() == target.id() && following.order() == target.order() =>
        {
            None
        }
        _ => None,
    };
    let following = match target.range().end().gap {
        InlineObjectGap::Between {
            preceding,
            following,
        } if preceding.id() == target.id() && preceding.order() == target.order() => {
            Some(following.id())
        }
        InlineObjectGap::After(preceding)
            if preceding.id() == target.id() && preceding.order() == target.order() =>
        {
            None
        }
        _ => None,
    };
    [preceding, following]
}

fn validate_successor_at(
    position: SourcePosition,
    object: SuccessorObject,
    moving: Option<InlineObjectId>,
) -> Result<(), MutationError> {
    if object.anchor() != position.byte_offset {
        return Err(MutationError::MalformedObjectChange);
    }
    let invalid_neighbor = |neighbor: crate::InlineObjectNeighbor| {
        neighbor.id() == object.id() || moving.is_some_and(|moving| neighbor.id() == moving)
    };
    let ordered = match position.gap {
        InlineObjectGap::NoObjects => true,
        InlineObjectGap::Before(following) => {
            !invalid_neighbor(following) && object.order() < following.order()
        }
        InlineObjectGap::Between {
            preceding,
            following,
        } => {
            !invalid_neighbor(preceding)
                && !invalid_neighbor(following)
                && preceding.order() < object.order()
                && object.order() < following.order()
        }
        InlineObjectGap::After(preceding) => {
            !invalid_neighbor(preceding) && preceding.order() < object.order()
        }
    };
    if !ordered {
        return Err(MutationError::MalformedObjectChange);
    }
    Ok(())
}

fn source_range_contains(outer: SourceRange, inner: SourceRange) -> bool {
    matches!(
        positions_cmp(outer.start(), inner.start()),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    ) && matches!(
        positions_cmp(inner.end(), outer.end()),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    )
}

fn source_range_contains_position(range: SourceRange, position: SourcePosition) -> bool {
    matches!(
        positions_cmp(range.start(), position),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    ) && matches!(
        positions_cmp(position, range.end()),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    )
}

fn positions_cmp(left: SourcePosition, right: SourcePosition) -> Option<std::cmp::Ordering> {
    left.compare_in_revision(right)
}
