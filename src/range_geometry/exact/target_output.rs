use gpui::{Pixels, Point, StreamingLayoutContinuation, StreamingLayoutFragment};

use crate::SourcePosition;

use super::{ActiveJob, ActiveKind, BlockTarget};

pub(super) fn fragment_intersects_target(
    fragment: &StreamingLayoutFragment,
    prior: StreamingLayoutContinuation,
    target: BlockTarget,
    anchor: Option<SourcePosition>,
    line_height: Pixels,
) -> bool {
    let window_start = target.block_offset;
    let window_end = target.block_offset + target.viewport_extent + target.overscan;
    let leading_context_start = (target.block_offset - target.overscan).max(Pixels::ZERO);
    let (start, end) = fragment_vertical_bounds(fragment, prior, line_height);
    (end > window_start && start < window_end)
        || (matches!(fragment, StreamingLayoutFragment::InlineObject(_))
            && end > leading_context_start
            && start < window_start)
        || (start >= window_start
            && anchor.is_some_and(|anchor| fragment_anchor_position(fragment, anchor).is_some()))
}

fn fragment_anchor_position(
    fragment: &StreamingLayoutFragment,
    anchor: SourcePosition,
) -> Option<Point<Pixels>> {
    let anchor = anchor.into();
    let mapped = match fragment {
        StreamingLayoutFragment::Text(fragment) => fragment
            .position_for_logical_position(anchor)
            .ok()
            .flatten(),
        StreamingLayoutFragment::OversizeAtom(fragment) => {
            fragment.position_for_logical_position(anchor)
        }
        StreamingLayoutFragment::InlineObject(fragment) => {
            fragment.position_for_logical_position(anchor)
        }
        StreamingLayoutFragment::Boundary(fragment) => {
            fragment.position_for_logical_position(anchor)
        }
    };
    mapped.or_else(|| {
        fragment_maps(fragment)
            .iter()
            .find(|map| map.logical_position == anchor)
            .map(|map| map.position)
    })
}

fn fragment_maps(fragment: &StreamingLayoutFragment) -> &[gpui::StreamingLayoutMap] {
    match fragment {
        StreamingLayoutFragment::Text(fragment) => fragment.maps(),
        StreamingLayoutFragment::OversizeAtom(fragment) => fragment.maps(),
        StreamingLayoutFragment::InlineObject(fragment) => fragment.maps(),
        StreamingLayoutFragment::Boundary(fragment) => fragment.maps(),
    }
}

pub(super) fn resolve_source_anchor(job: &mut ActiveJob, fragments: &[StreamingLayoutFragment]) {
    let ActiveKind::Target { anchor, .. } = &mut job.kind else {
        return;
    };
    let Some(source_anchor) = *anchor else {
        return;
    };
    let Some(_) = fragments
        .iter()
        .find_map(|fragment| fragment_anchor_position(fragment, source_anchor))
    else {
        return;
    };
    *anchor = None;
}

fn fragment_vertical_bounds(
    fragment: &StreamingLayoutFragment,
    prior: StreamingLayoutContinuation,
    line_height: Pixels,
) -> (Pixels, Pixels) {
    match fragment {
        StreamingLayoutFragment::Text(fragment) => {
            let start = fragment
                .maps()
                .iter()
                .map(|map| map.position.y)
                .min_by(|left, right| left.partial_cmp(right).unwrap())
                .unwrap_or(prior.block_offset);
            let last = fragment
                .maps()
                .iter()
                .map(|map| map.position.y)
                .max_by(|left, right| left.partial_cmp(right).unwrap())
                .unwrap_or(prior.block_offset);
            let extent = if last == prior.block_offset {
                prior.line_block_extent.max(line_height)
            } else {
                line_height
            };
            (start, last + extent)
        }
        StreamingLayoutFragment::OversizeAtom(fragment) => (
            fragment.bounds.origin.y,
            fragment.bounds.origin.y + fragment.bounds.size.height,
        ),
        StreamingLayoutFragment::InlineObject(fragment) => (
            fragment.bounds.origin.y,
            fragment.bounds.origin.y + fragment.bounds.size.height,
        ),
        StreamingLayoutFragment::Boundary(fragment) => {
            let start = fragment
                .maps()
                .iter()
                .map(|map| map.position.y)
                .min_by(|left, right| left.partial_cmp(right).unwrap())
                .unwrap_or(prior.block_offset);
            let end = fragment
                .maps()
                .iter()
                .map(|map| map.position.y)
                .max_by(|left, right| left.partial_cmp(right).unwrap())
                .unwrap_or(prior.block_offset);
            (start, end + line_height)
        }
    }
}

pub(super) fn update_target_source(
    job: &mut ActiveJob,
    fragments: &[StreamingLayoutFragment],
    continuation: StreamingLayoutContinuation,
) {
    let ActiveKind::Target { target, .. } = job.kind else {
        return;
    };
    if job.scanner.target_source.is_some() {
        return;
    }
    for fragment in fragments {
        let maps: &[gpui::StreamingLayoutMap] = match fragment {
            StreamingLayoutFragment::Text(fragment) => fragment.maps(),
            StreamingLayoutFragment::OversizeAtom(fragment) => fragment.maps(),
            StreamingLayoutFragment::InlineObject(fragment) => fragment.maps(),
            StreamingLayoutFragment::Boundary(fragment) => fragment.maps(),
        };
        for map in maps {
            if map.position.y > job.scanner.target_line_block {
                if target.block_offset < map.position.y {
                    job.scanner.target_source = Some(job.scanner.target_line_position);
                    return;
                }
                job.scanner.target_line_block = map.position.y;
                job.scanner.target_line_position = map
                    .logical_position
                    .try_into()
                    .expect("accepted GPUI map position is source-compatible");
            }
        }
    }
    if continuation.block_offset > target.block_offset {
        job.scanner.target_source = Some(job.scanner.target_line_position);
    } else {
        if continuation.block_offset > job.scanner.target_line_block {
            job.scanner.target_line_block = continuation.block_offset;
            job.scanner.target_line_position = continuation
                .next_position
                .try_into()
                .expect("accepted GPUI continuation is source-compatible");
        }
        if target.block_offset >= job.scanner.target_line_block
            && target.block_offset < continuation.block_offset + continuation.line_block_extent
        {
            job.scanner.target_source = Some(job.scanner.target_line_position);
        }
    }
}

pub(super) fn finish_target_source(job: &mut ActiveJob) {
    if matches!(job.kind, ActiveKind::Target { .. }) && job.scanner.target_source.is_none() {
        job.scanner.target_source = Some(job.scanner.target_line_position);
    }
}
