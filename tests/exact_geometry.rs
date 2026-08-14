use std::sync::Arc;

use gpui::{
    FontFallbacks, FontFeatures, SharedString, StreamingLayoutBinding, StreamingLayoutFragment,
    StreamingLayoutLimits, TestAppContext, TextRun, WindowTextSystem, black, font, px,
};
use gpui_text_input::{
    AtomFact, AtomId, BindingId, BlockTarget, ByteOffset, ByteRange, ExactGeometryError,
    ExactGeometryLimits, ExactGeometryOwner, ExactGeometryProgress, ExactGeometryRelease,
    GeometryJobId, GeometryJobKey, LogicalExtent, PageEdgeFact, PageId, PageRequestId,
    RangeBinding, RangePage, SourceRevision, StreamingGeometryStyle, StreamingOversizePresentation,
};

fn layout(segment_bytes: usize, wrap_width: f32) -> StreamingLayoutBinding {
    StreamingLayoutBinding {
        input_id: 41,
        segment_policy_id: 73,
        wrap_width: px(wrap_width),
        font_size: px(10.),
        line_height: px(14.),
        limits: StreamingLayoutLimits {
            segment_bytes,
            runs: 8,
            decorations: 8,
            glyphs: 256,
            wraps: 128,
            maps: 257,
            fragments: 1,
            retained_bytes: 128 * 1024,
        },
    }
}

fn style() -> StreamingGeometryStyle {
    style_with_font(font(".SystemUIFont"), "")
}

fn style_with_font(font: gpui::Font, presentation: &str) -> StreamingGeometryStyle {
    let run = TextRun {
        len: 0,
        font: font.clone(),
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let oversize_runs = if presentation.is_empty() {
        vec![]
    } else {
        vec![TextRun {
            len: presentation.len(),
            font,
            color: black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }]
    };
    StreamingGeometryStyle::new(
        run,
        StreamingOversizePresentation::new(
            SharedString::new(Arc::<str>::from(presentation)),
            oversize_runs,
            px(12.),
            px(14.),
            px(0.),
            None,
        ),
    )
}

fn binding(source: &str, revision: u64) -> RangeBinding {
    RangeBinding::new(
        BindingId::new(9),
        SourceRevision::new(revision),
        LogicalExtent::new(
            source.len() as u64,
            if source.is_empty() {
                0
            } else {
                source.matches('\n').count() as u64 + 1
            },
        ),
    )
}

fn owner_with(
    source: &str,
    segment_bytes: usize,
    wrap_width: f32,
    checkpoint_cap: usize,
    byte_cap: usize,
    output_window_items: usize,
    geometry_style: StreamingGeometryStyle,
) -> Result<ExactGeometryOwner, gpui_text_input::ExactGeometryError> {
    owner_with_retained_items(
        source,
        segment_bytes,
        wrap_width,
        checkpoint_cap,
        byte_cap,
        output_window_items.saturating_mul(1024),
        geometry_style,
    )
}

fn owner_with_retained_items(
    source: &str,
    segment_bytes: usize,
    wrap_width: f32,
    checkpoint_cap: usize,
    byte_cap: usize,
    retained_items: usize,
    geometry_style: StreamingGeometryStyle,
) -> Result<ExactGeometryOwner, gpui_text_input::ExactGeometryError> {
    ExactGeometryOwner::new(
        binding(source, 1),
        layout(segment_bytes, wrap_width),
        geometry_style,
        ExactGeometryLimits::new(256, checkpoint_cap, byte_cap, retained_items).unwrap(),
    )
}

fn owner(
    source: &str,
    segment_bytes: usize,
    checkpoint_cap: usize,
    byte_cap: usize,
    output_items: usize,
) -> ExactGeometryOwner {
    owner_with(
        source,
        segment_bytes,
        24.,
        checkpoint_cap,
        byte_cap,
        output_items,
        style(),
    )
    .unwrap()
}

fn page(
    owner: &mut ExactGeometryOwner,
    job: GeometryJobKey,
    source: &str,
    start: usize,
    end: usize,
    id: u64,
) -> RangePage {
    page_with_atoms(owner, job, source, start, end, id, vec![])
}

fn page_with_atoms(
    owner: &mut ExactGeometryOwner,
    job: GeometryJobKey,
    source: &str,
    start: usize,
    end: usize,
    id: u64,
    atoms: Vec<AtomFact>,
) -> RangePage {
    let range = ByteRange::from_u64(start as u64, end as u64).unwrap();
    let request = owner.request_page(job, PageRequestId::new(id)).unwrap();
    RangePage::new(
        PageId::new(id),
        request.key(),
        range,
        source[start..end].to_owned(),
        atoms,
        if start == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if end == source.len() {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        end == source.len(),
    )
    .unwrap()
}

fn start_index(owner: &mut ExactGeometryOwner, id: u64) -> GeometryJobKey {
    let start = owner.start_index(GeometryJobId::new(id)).unwrap();
    assert_eq!(start.progress(), ExactGeometryProgress::Scanning);
    start.key()
}

fn scan_index(
    text_system: &WindowTextSystem,
    source: &str,
    partitions: &[usize],
    segment_bytes: usize,
    checkpoint_cap: usize,
    byte_cap: usize,
    output_items: usize,
) -> ExactGeometryOwner {
    let mut owner = owner(
        source,
        segment_bytes,
        checkpoint_cap,
        byte_cap,
        output_items,
    );
    let job = start_index(&mut owner, 1);
    let mut start = 0;
    for (ix, &end) in partitions.iter().enumerate() {
        let page = page(&mut owner, job, source, start, end, ix as u64 + 1);
        let progress = owner.admit_page(job, &page, text_system).unwrap();
        assert_eq!(
            progress.progress(),
            if end == source.len() {
                ExactGeometryProgress::IndexComplete
            } else {
                ExactGeometryProgress::Scanning
            }
        );
        start = end;
    }
    owner
}

fn drive_ascii_job(
    owner: &mut ExactGeometryOwner,
    text_system: &WindowTextSystem,
    source: &str,
    job: GeometryJobKey,
    mut start: usize,
    page_bytes: usize,
    first_request_id: u64,
) -> ExactGeometryProgress {
    let mut request_id = first_request_id;
    loop {
        let end = start.saturating_add(page_bytes).min(source.len());
        let page = page(owner, job, source, start, end, request_id);
        let admission = owner.admit_page(job, &page, text_system).unwrap();
        if admission.progress() != ExactGeometryProgress::Scanning {
            return admission.progress();
        }
        start = end;
        request_id += 1;
    }
}

fn fragment_facts(
    fragments: &[StreamingLayoutFragment],
) -> Vec<(std::ops::Range<u64>, Vec<(u64, u32, u32)>)> {
    fragments
        .iter()
        .map(|fragment| {
            let (range, maps) = match fragment {
                StreamingLayoutFragment::Text(fragment) => {
                    (fragment.logical_range(), fragment.maps().as_ref())
                }
                StreamingLayoutFragment::OversizeAtom(fragment) => {
                    (fragment.logical_range.clone(), fragment.maps().as_ref())
                }
            };
            (
                range,
                maps.iter()
                    .map(|map| {
                        (
                            map.logical_offset,
                            f32::from(map.position.x).to_bits(),
                            f32::from(map.position.y).to_bits(),
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

fn with_text_system(test: &mut TestAppContext, f: impl FnOnce(&WindowTextSystem)) {
    let cx = test.add_empty_window();
    cx.update(|window, _| f(window.text_system()));
}

#[path = "exact_geometry/canonical.rs"]
mod canonical;
#[path = "exact_geometry/capacity_lifecycle.rs"]
mod capacity_lifecycle;
#[path = "exact_geometry/precontext.rs"]
mod precontext;
#[path = "exact_geometry/precontext_atoms.rs"]
mod precontext_atoms;
#[path = "exact_geometry/release_peak.rs"]
mod release_peak;
