use std::mem::size_of;

use super::*;

pub(in crate::range_geometry::exact) fn index_release(
    index: Box<ExactGeometryIndex>,
) -> ExactGeometryRelease {
    let mut counts = ExactGeometryCounts::default();
    counts.publication_bytes = size_of::<ExactGeometryIndex>();
    counts.publication_items = 1;
    counts.checkpoints = index.checkpoints.len();
    counts.checkpoint_bytes = accounting::checkpoint_record_bytes(index.checkpoints.len());
    ExactGeometryRelease {
        jobs: vec![index.key],
        counts,
        ..Default::default()
    }
}

pub(in crate::range_geometry::exact) fn target_release(
    target: Box<BlockTargetPublication>,
) -> ExactGeometryRelease {
    let mut counts = ExactGeometryCounts::default();
    counts.publication_bytes = size_of::<BlockTargetPublication>();
    counts.publication_items = 1;
    counts.output_items = target.item_charge.total().unwrap_or(usize::MAX);
    counts.output_record_bytes = accounting::fragment_record_bytes(target.fragments.len());
    counts.output_payload_bytes = target.charge.total().unwrap_or(usize::MAX);
    ExactGeometryRelease {
        jobs: vec![target.key],
        counts,
        ..Default::default()
    }
}

pub(in crate::range_geometry::exact) fn merge_release(
    mut left: ExactGeometryRelease,
    right: ExactGeometryRelease,
) -> ExactGeometryRelease {
    left.jobs.extend(right.jobs);
    left.pages.extend(right.pages);
    left.jobs.sort();
    left.jobs.dedup();
    add_counts(&mut left.counts, right.counts);
    left
}

fn add_counts(left: &mut ExactGeometryCounts, right: ExactGeometryCounts) {
    macro_rules! add {
        ($field:ident) => {
            left.$field = left.$field.saturating_add(right.$field);
        };
    }
    add!(owner_bytes);
    add!(owner_items);
    add!(input_bytes);
    add!(input_items);
    add!(desired_target_bytes);
    add!(desired_target_items);
    add!(active_job_bytes);
    add!(active_job_items);
    add!(pending_page_bytes);
    add!(pending_page_items);
    add!(scan_buffer_bytes);
    add!(scan_buffer_items);
    add!(active_atom_bytes);
    add!(active_atom_items);
    add!(checkpoints);
    add!(checkpoint_bytes);
    add!(continuation_bytes);
    add!(continuation_items);
    add!(output_items);
    add!(output_record_bytes);
    add!(output_payload_bytes);
    add!(publication_bytes);
    add!(publication_items);
}
