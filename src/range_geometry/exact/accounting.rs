use std::mem::{size_of, size_of_val};

use gpui::{
    StreamingLayoutBinding, StreamingLayoutCharge, StreamingLayoutContinuation,
    StreamingLayoutFragment, StreamingLayoutItemCharge, TextRun,
};

use crate::{PageRequestKey, StreamingGeometryStyle};

use super::{
    ActiveJob, BlockTargetPublication, DeferredObject, DesiredTarget, ExactGeometryCheckpoint,
    ExactGeometryCounts, ExactGeometryError, ExactGeometryIndex, ExactGeometryOwner, OwnerInputs,
    PendingInput,
};

pub(super) fn add_fragment_charge(
    left: StreamingLayoutCharge,
    mut right: StreamingLayoutCharge,
) -> Result<StreamingLayoutCharge, ExactGeometryError> {
    right.continuation = 0;
    macro_rules! add {
        ($field:ident) => {
            left.$field
                .checked_add(right.$field)
                .ok_or(ExactGeometryError::CapacityExceeded)?
        };
    }
    Ok(StreamingLayoutCharge {
        segment_text: add!(segment_text),
        runs: add!(runs),
        decorations: add!(decorations),
        glyphs: add!(glyphs),
        wrap_facts: add!(wrap_facts),
        maps: add!(maps),
        objects: add!(objects),
        fragments: add!(fragments),
        continuation: 0,
    })
}

pub(super) fn add_fragment_item_charge(
    left: StreamingLayoutItemCharge,
    mut right: StreamingLayoutItemCharge,
) -> Result<StreamingLayoutItemCharge, ExactGeometryError> {
    right.continuations = 0;
    macro_rules! add {
        ($field:ident) => {
            left.$field
                .checked_add(right.$field)
                .ok_or(ExactGeometryError::CapacityExceeded)?
        };
    }
    Ok(StreamingLayoutItemCharge {
        text_payloads: add!(text_payloads),
        style_runs: add!(style_runs),
        shaped_runs: add!(shaped_runs),
        glyphs: add!(glyphs),
        decorations: add!(decorations),
        wrap_facts: add!(wrap_facts),
        maps: add!(maps),
        positions: add!(positions),
        gap_witnesses: add!(gap_witnesses),
        object_ids: add!(object_ids),
        object_orders: add!(object_orders),
        fragments: add!(fragments),
        continuations: 0,
    })
}

pub(super) const fn ordinary_continuation_items() -> usize {
    // One continuation retains its record, next composite position, and no-object gap witness.
    3
}

pub(super) fn owner_counts(owner: &ExactGeometryOwner) -> ExactGeometryCounts {
    counts(
        owner.inputs.as_deref(),
        owner.active.as_deref(),
        owner.desired_target.as_deref(),
        owner.index.as_deref(),
        owner.target.as_deref(),
    )
}

pub(super) fn fixed_bytes_without_active(owner: &ExactGeometryOwner) -> usize {
    fixed_counts_without_active(owner).total_bytes()
}

pub(super) fn fixed_counts_without_active(owner: &ExactGeometryOwner) -> ExactGeometryCounts {
    counts(
        owner.inputs.as_deref(),
        None,
        owner.desired_target.as_deref(),
        owner.index.as_deref(),
        owner.target.as_deref(),
    )
}

pub(super) const fn fragment_record_bytes(items: usize) -> usize {
    items.saturating_mul(size_of::<StreamingLayoutFragment>())
}

pub(super) const fn checkpoint_record_bytes(items: usize) -> usize {
    items.saturating_mul(size_of::<ExactGeometryCheckpoint>())
}

pub(super) const fn index_publication_record_bytes(items: usize) -> usize {
    size_of::<ExactGeometryIndex>().saturating_add(checkpoint_record_bytes(items))
}

pub(super) const fn target_publication_record_bytes(items: usize) -> usize {
    size_of::<BlockTargetPublication>().saturating_add(fragment_record_bytes(items))
}

pub(super) fn counts_with_index_candidate(
    owner: &ExactGeometryOwner,
    candidate: &ExactGeometryIndex,
) -> ExactGeometryCounts {
    let mut result = owner_counts(owner);
    add_index(&mut result, candidate);
    result
}

pub(super) fn counts_with_target_candidate(
    owner: &ExactGeometryOwner,
    candidate: &BlockTargetPublication,
) -> ExactGeometryCounts {
    let mut result = owner_counts(owner);
    add_target(&mut result, candidate);
    result
}

pub(super) fn counts_with_input_candidate(
    owner: &ExactGeometryOwner,
    candidate: &OwnerInputs,
) -> ExactGeometryCounts {
    add_counts(owner_counts(owner), input_counts(candidate))
}

pub(super) fn active_bytes(active: &ActiveJob) -> usize {
    active_counts(active).total_bytes()
}

pub(super) fn active_counts(active: &ActiveJob) -> ExactGeometryCounts {
    let mut counts = ExactGeometryCounts::default();
    add_active(&mut counts, active);
    counts
}

pub(super) fn ensure_active(active: &mut ActiveJob) -> Result<(), ExactGeometryError> {
    let retained = active_bytes(active);
    if retained > active.retained_capacity {
        Err(ExactGeometryError::CapacityExceeded)
    } else {
        Ok(())
    }
}

pub(super) fn ensure_owner(owner: &ExactGeometryOwner) -> Result<(), ExactGeometryError> {
    let counts = owner_counts(owner);
    if counts.total_bytes() > owner.limits.max_retained_bytes
        || counts.total_items() > owner.limits.max_retained_items
    {
        Err(ExactGeometryError::CapacityExceeded)
    } else {
        Ok(())
    }
}

pub(super) fn counts(
    inputs: Option<&OwnerInputs>,
    active: Option<&ActiveJob>,
    desired: Option<&DesiredTarget>,
    index: Option<&ExactGeometryIndex>,
    target: Option<&BlockTargetPublication>,
) -> ExactGeometryCounts {
    let mut counts = ExactGeometryCounts {
        owner_items: 1,
        owner_bytes: size_of::<ExactGeometryOwner>(),
        ..Default::default()
    };
    if let Some(inputs) = inputs {
        counts.input_bytes = size_of::<OwnerInputs>().saturating_add(style_payload_bytes(inputs));
        counts.input_items = input_items(inputs);
    }
    if desired.is_some() {
        counts.desired_target_items = 1;
        counts.desired_target_bytes = size_of::<DesiredTarget>();
    }
    if let Some(active) = active {
        add_active(&mut counts, active);
    }
    if let Some(index) = index {
        add_index(&mut counts, index);
    }
    if let Some(target) = target {
        add_target(&mut counts, target);
    }
    counts
}

pub(super) fn input_counts(inputs: &OwnerInputs) -> ExactGeometryCounts {
    ExactGeometryCounts {
        input_bytes: size_of::<OwnerInputs>().saturating_add(style_payload_bytes(inputs)),
        input_items: input_items(inputs),
        ..Default::default()
    }
}

pub(super) fn desired_counts() -> ExactGeometryCounts {
    ExactGeometryCounts {
        desired_target_items: 1,
        desired_target_bytes: size_of::<DesiredTarget>(),
        ..Default::default()
    }
}

pub(super) fn target_counts(target: &BlockTargetPublication) -> ExactGeometryCounts {
    let mut result = ExactGeometryCounts::default();
    add_target(&mut result, target);
    result
}

pub(super) fn add_counts(
    mut left: ExactGeometryCounts,
    right: ExactGeometryCounts,
) -> ExactGeometryCounts {
    macro_rules! add {
        ($field:ident) => {
            left.$field = left.$field.saturating_add(right.$field)
        };
    }
    add!(owner_items);
    add!(owner_bytes);
    add!(input_items);
    add!(input_bytes);
    add!(desired_target_items);
    add!(desired_target_bytes);
    add!(active_job_items);
    add!(active_job_bytes);
    add!(pending_page_items);
    add!(pending_page_bytes);
    add!(pending_object_page_items);
    add!(pending_object_page_bytes);
    add!(scan_buffer_items);
    add!(scan_buffer_bytes);
    add!(active_atom_items);
    add!(active_atom_bytes);
    add!(deferred_object_items);
    add!(deferred_object_bytes);
    add!(checkpoints);
    add!(checkpoint_bytes);
    add!(continuation_items);
    add!(continuation_bytes);
    add!(output_items);
    add!(output_record_bytes);
    add!(output_payload_bytes);
    add!(publication_items);
    add!(publication_bytes);
    left
}

fn add_index(counts: &mut ExactGeometryCounts, index: &ExactGeometryIndex) {
    counts.publication_items = counts.publication_items.saturating_add(1);
    counts.publication_bytes = counts
        .publication_bytes
        .saturating_add(size_of::<ExactGeometryIndex>());
    counts.checkpoints = counts.checkpoints.saturating_add(index.checkpoints.len());
    counts.checkpoint_bytes = counts.checkpoint_bytes.saturating_add(
        index
            .checkpoints
            .len()
            .saturating_mul(size_of::<ExactGeometryCheckpoint>()),
    );
}

fn add_target(counts: &mut ExactGeometryCounts, target: &BlockTargetPublication) {
    counts.publication_items = counts.publication_items.saturating_add(1);
    counts.publication_bytes = counts
        .publication_bytes
        .saturating_add(size_of::<BlockTargetPublication>());
    add_output(
        counts,
        target.item_charge.total().unwrap_or(usize::MAX),
        target.fragments.len(),
        target.charge.total().unwrap_or(usize::MAX),
    );
}

fn add_active(counts: &mut ExactGeometryCounts, active: &ActiveJob) {
    counts.active_job_items = counts.active_job_items.saturating_add(2);
    counts.active_job_bytes = counts.active_job_bytes.saturating_add(
        size_of::<ActiveJob>().saturating_sub(size_of::<StreamingLayoutContinuation>()),
    );
    counts.continuation_bytes = counts
        .continuation_bytes
        .saturating_add(size_of::<StreamingLayoutContinuation>());
    match active.pending.as_deref() {
        Some(PendingInput::Text(_)) => {
            counts.pending_page_items = counts.pending_page_items.saturating_add(1);
            counts.pending_page_bytes = counts
                .pending_page_bytes
                .saturating_add(size_of::<PageRequestKey>());
        }
        Some(PendingInput::Object(_)) => {
            counts.pending_object_page_items = counts.pending_object_page_items.saturating_add(1);
            counts.pending_object_page_bytes = counts
                .pending_object_page_bytes
                .saturating_add(size_of::<crate::ObjectRequestKey>());
        }
        None => {}
    }
    counts.scan_buffer_bytes = counts
        .scan_buffer_bytes
        .saturating_add(active.scanner.segment_text.capacity())
        .saturating_add(
            active
                .scanner
                .grapheme_text
                .as_ref()
                .map_or(0, String::capacity),
        );
    counts.scan_buffer_items = counts
        .scan_buffer_items
        .saturating_add(1)
        .saturating_add(usize::from(active.scanner.grapheme_text.is_some()));
    if let Some(atom) = active.scanner.active_atom.as_deref() {
        counts.active_atom_items = counts.active_atom_items.saturating_add(1);
        counts.active_atom_bytes = counts.active_atom_bytes.saturating_add(size_of_val(atom));
    }
    if let Some(object) = active.scanner.deferred_object.as_deref() {
        counts.deferred_object_items = counts.deferred_object_items.saturating_add(4);
        counts.deferred_object_bytes = counts.deferred_object_bytes.saturating_add(
            size_of::<DeferredObject>()
                .saturating_sub(size_of::<crate::InlineObjectFact>())
                .saturating_add(object.fact.retained_bytes().unwrap_or(usize::MAX)),
        );
    }
    counts.checkpoints = counts
        .checkpoints
        .saturating_add(active.scanner.checkpoints.capacity());
    counts.checkpoint_bytes = counts.checkpoint_bytes.saturating_add(
        active
            .scanner
            .checkpoints
            .capacity()
            .saturating_mul(size_of::<ExactGeometryCheckpoint>()),
    );
    counts.continuation_items = counts
        .continuation_items
        .saturating_add(active.scanner.continuation_items);
    add_output(
        counts,
        active
            .scanner
            .output_item_charge
            .total()
            .unwrap_or(usize::MAX),
        active.scanner.fragments.capacity(),
        active.scanner.output_charge.total().unwrap_or(usize::MAX),
    );
}

fn add_output(
    counts: &mut ExactGeometryCounts,
    semantic_items: usize,
    records: usize,
    payload: usize,
) {
    counts.output_items = counts.output_items.saturating_add(semantic_items);
    counts.output_record_bytes = counts
        .output_record_bytes
        .saturating_add(records.saturating_mul(size_of::<StreamingLayoutFragment>()));
    counts.output_payload_bytes = counts.output_payload_bytes.saturating_add(payload);
}

fn input_items(inputs: &OwnerInputs) -> usize {
    input_items_for_style(&inputs.style)
}

pub(super) fn initial_owner_counts(
    layout: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
) -> ExactGeometryCounts {
    let _ = layout;
    ExactGeometryCounts {
        owner_items: 1,
        owner_bytes: size_of::<ExactGeometryOwner>(),
        input_bytes: size_of::<OwnerInputs>().saturating_add(style_payload_bytes_for_style(style)),
        input_items: input_items_for_style(style),
        ..Default::default()
    }
}

pub(super) fn layout_style_counts(
    layout: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
) -> (usize, usize) {
    let _ = layout;
    (
        size_of::<StreamingLayoutBinding>()
            .saturating_add(size_of::<StreamingGeometryStyle>())
            .saturating_add(style_payload_bytes_for_style(style)),
        input_items_for_style(style).saturating_sub(2),
    )
}

fn input_items_for_style(style: &StreamingGeometryStyle) -> usize {
    // OwnerInputs, binding, layout binding, style, oversize presentation, its bounded presentation
    // payload, and its run collection are distinct records. Each run then owns its run record,
    // family payload, feature collection and entries, plus an optional fallback collection and
    // entries.
    7usize
        .saturating_add(text_run_metadata_items(&style.text_run))
        .saturating_add(
            style
                .oversize
                .runs
                .iter()
                .map(text_run_metadata_items)
                .fold(0usize, usize::saturating_add),
        )
}

fn text_run_metadata_items(run: &TextRun) -> usize {
    let features = run.font.features.tag_value_list();
    let fallback_items = run.font.fallbacks.as_ref().map_or(0, |fallbacks| {
        1usize.saturating_add(fallbacks.fallback_list().len())
    });
    3usize
        .saturating_add(features.len())
        .saturating_add(fallback_items)
}

fn style_payload_bytes(inputs: &OwnerInputs) -> usize {
    style_payload_bytes_for_style(&inputs.style)
}

fn style_payload_bytes_for_style(style: &StreamingGeometryStyle) -> usize {
    let mut bytes = text_run_payload_bytes(&style.text_run);
    bytes = bytes.saturating_add(style.oversize.presentation.len());
    bytes = bytes.saturating_add(
        style
            .oversize
            .runs
            .capacity()
            .saturating_mul(size_of::<TextRun>()),
    );
    for run in &style.oversize.runs {
        bytes = bytes.saturating_add(text_run_payload_bytes(run));
    }
    bytes
}

fn text_run_payload_bytes(run: &TextRun) -> usize {
    let mut bytes = run.font.family.len();
    let features = run.font.features.tag_value_list();
    bytes = bytes.saturating_add(size_of::<Vec<(String, u32)>>());
    for (tag, _) in features {
        bytes = bytes
            .saturating_add(size_of::<(String, u32)>())
            .saturating_add(tag.len());
    }
    if let Some(fallbacks) = &run.font.fallbacks {
        bytes = bytes.saturating_add(size_of::<Vec<String>>());
        for family in fallbacks.fallback_list() {
            bytes = bytes
                .saturating_add(size_of::<String>())
                .saturating_add(family.len());
        }
    }
    bytes
}
