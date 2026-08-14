use gpui::{Bounds, Pixels, Point, SharedString, StreamingLayoutFragment, StreamingLayoutHit, px};

use crate::{
    AtomFact, BlockTargetPublication, ByteOffset, ByteRange, GeometryKey, GeometryQuality,
    RangeBinding, RangePage,
};

use super::{DesiredSurface, RangeSelection};

/// Exact retained byte and semantic-record charge for one coherent surface.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangeSurfaceCharge {
    pub bytes: usize,
    pub items: usize,
}

impl RangeSurfaceCharge {
    pub fn replacement_peak(self, candidate: Self) -> Self {
        Self {
            bytes: self.bytes.saturating_add(candidate.bytes),
            items: self.items.saturating_add(candidate.items),
        }
    }
}

/// One atomically published logical and visual range-backed widget surface.
#[derive(Debug)]
pub struct CoherentRangeSurface {
    binding: RangeBinding,
    geometry: GeometryKey,
    pages: Box<[RangePage]>,
    selection: RangeSelection,
    composition: Option<ByteRange>,
    scroll_source: ByteOffset,
    scroll_block: Pixels,
    scroll_intra_anchor: Pixels,
    viewport: ByteRange,
    overscan: ByteRange,
    target: BlockTargetPublication,
    quality: GeometryQuality,
    visual_lines: u64,
    content_height: Pixels,
    selection_geometry: Box<[Bounds<Pixels>]>,
    composition_geometry: Box<[Bounds<Pixels>]>,
    caret_geometry: Option<Bounds<Pixels>>,
    placeholder: Option<SharedString>,
    charge: RangeSurfaceCharge,
}

impl CoherentRangeSurface {
    pub(super) fn new(
        binding: RangeBinding,
        pages: Vec<RangePage>,
        desired: DesiredSurface,
        target: BlockTargetPublication,
        visual_lines: u64,
        content_height: Pixels,
        line_height: Pixels,
        wrap_width: Pixels,
        placeholder: SharedString,
    ) -> Result<Self, crate::RangeTextInputError> {
        let viewport = ByteRange::new(target.target_source(), target.source_end())?;
        let overscan = ByteRange::new(target.predecessor(), target.source_end())?;
        let page_bytes = pages
            .iter()
            .try_fold(0usize, |total, page| {
                total.checked_add(page.retained_charge().bytes())
            })
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let page_items = pages
            .iter()
            .try_fold(0usize, |total, page| {
                total.checked_add(page.retained_charge().items())
            })
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let fragment_bytes = target
            .charge()
            .total()
            .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?;
        let fragment_items = target
            .item_charge()
            .total()
            .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?;
        let selection_geometry = bounds_for_fragments(
            target.fragments(),
            desired.selection.range(),
            line_height,
            wrap_width,
        );
        let composition_geometry = desired.composition.map_or_else(Vec::new, |range| {
            bounds_for_fragments(target.fragments(), range, line_height, wrap_width)
        });
        let caret_geometry = position_for_fragments(target.fragments(), desired.selection.head)
            .map(|origin| Bounds::new(origin, gpui::size(px(2.), line_height)));
        let geometry_items = selection_geometry
            .len()
            .checked_add(composition_geometry.len())
            .and_then(|value| value.checked_add(usize::from(caret_geometry.is_some())))
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let geometry_bytes = geometry_items
            .checked_mul(std::mem::size_of::<Bounds<Pixels>>())
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let placeholder =
            (binding.extent().byte_len() == 0 && !placeholder.is_empty()).then_some(placeholder);
        let placeholder_bytes = placeholder.as_ref().map_or(0, |placeholder| {
            std::mem::size_of::<SharedString>().saturating_add(placeholder.len())
        });
        let placeholder_items = usize::from(placeholder.is_some());
        let charge = RangeSurfaceCharge {
            bytes: std::mem::size_of::<Self>()
                .checked_add(page_bytes)
                .and_then(|value| value.checked_add(fragment_bytes))
                .and_then(|value| value.checked_add(geometry_bytes))
                .and_then(|value| value.checked_add(placeholder_bytes))
                .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
            items: 1usize
                .checked_add(page_items)
                .and_then(|value| value.checked_add(fragment_items))
                .and_then(|value| value.checked_add(geometry_items))
                .and_then(|value| value.checked_add(placeholder_items))
                .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
        };
        let scroll_source = if desired.preserve_scroll_anchor {
            desired.scroll.source
        } else {
            target.target_source()
        };
        let anchor_block = target
            .fragments()
            .iter()
            .find_map(|fragment| match fragment {
                StreamingLayoutFragment::Text(fragment) => {
                    let local = usize::try_from(
                        scroll_source
                            .get()
                            .checked_sub(fragment.logical_range().start)?,
                    )
                    .ok()?;
                    fragment
                        .position_for_index(local)
                        .ok()
                        .flatten()
                        .map(|position| position.y)
                }
                StreamingLayoutFragment::OversizeAtom(fragment) => fragment
                    .position_for_logical_offset(scroll_source.get())
                    .map(|position| position.y),
            })
            .or_else(|| {
                (binding.extent().byte_len() == 0 && scroll_source.get() == 0)
                    .then_some(Pixels::ZERO)
            })
            .ok_or(crate::RangeTextInputError::IncompleteSurface)?;
        let scroll_intra_anchor = if desired.preserve_scroll_anchor {
            desired.scroll.intra_anchor
        } else {
            (desired.target_block - anchor_block).max(Pixels::ZERO)
        };
        let scroll_block = (anchor_block + scroll_intra_anchor).max(Pixels::ZERO);
        Ok(Self {
            binding,
            geometry: target.key().geometry(),
            pages: pages.into_boxed_slice(),
            selection: desired.selection,
            composition: desired.composition,
            scroll_source,
            scroll_block,
            scroll_intra_anchor,
            viewport,
            overscan,
            target,
            quality: GeometryQuality::Exact,
            visual_lines,
            content_height,
            selection_geometry: selection_geometry.into_boxed_slice(),
            composition_geometry: composition_geometry.into_boxed_slice(),
            caret_geometry,
            placeholder,
            charge,
        })
    }

    pub const fn binding(&self) -> RangeBinding {
        self.binding
    }
    pub const fn geometry_key(&self) -> GeometryKey {
        self.geometry
    }
    pub const fn selection(&self) -> RangeSelection {
        self.selection
    }
    pub const fn composition(&self) -> Option<ByteRange> {
        self.composition
    }
    pub const fn caret(&self) -> ByteOffset {
        self.selection.head
    }
    pub const fn scroll_source(&self) -> ByteOffset {
        self.scroll_source
    }
    pub const fn scroll_block(&self) -> Pixels {
        self.scroll_block
    }
    pub const fn scroll_intra_anchor(&self) -> Pixels {
        self.scroll_intra_anchor
    }
    pub const fn viewport(&self) -> ByteRange {
        self.viewport
    }
    pub const fn overscan(&self) -> ByteRange {
        self.overscan
    }
    pub const fn quality(&self) -> GeometryQuality {
        self.quality
    }
    pub const fn visual_lines(&self) -> u64 {
        self.visual_lines
    }
    pub const fn content_height(&self) -> Pixels {
        self.content_height
    }
    pub const fn charge(&self) -> RangeSurfaceCharge {
        self.charge
    }
    pub fn pages(&self) -> &[RangePage] {
        &self.pages
    }
    pub fn fragments(&self) -> &[StreamingLayoutFragment] {
        self.target.fragments()
    }
    pub fn placeholder(&self) -> Option<&SharedString> {
        self.placeholder.as_ref()
    }

    pub fn atom_at(&self, offset: ByteOffset) -> Option<&AtomFact> {
        self.pages
            .iter()
            .flat_map(|page| page.atoms())
            .find(|atom| atom.global_range().contains_offset(offset))
    }

    pub fn position_for_offset(&self, offset: ByteOffset) -> Option<Point<Pixels>> {
        if self.binding.extent().byte_len() == 0 && offset.get() == 0 {
            return Some(gpui::point(Pixels::ZERO, Pixels::ZERO));
        }
        let offset = offset.get();
        for fragment in self.fragments() {
            match fragment {
                StreamingLayoutFragment::Text(fragment) => {
                    let range = fragment.logical_range();
                    if range.contains(&offset) || offset == range.end {
                        let local = usize::try_from(offset.checked_sub(range.start)?).ok()?;
                        if let Ok(Some(position)) = fragment.position_for_index(local) {
                            return Some(position);
                        }
                    }
                }
                StreamingLayoutFragment::OversizeAtom(fragment) => {
                    if let Some(position) = fragment.position_for_logical_offset(offset) {
                        return Some(position);
                    }
                }
            }
        }
        None
    }

    pub fn hit_test(&self, position: Point<Pixels>) -> Option<ByteOffset> {
        let mut last = None;
        for fragment in self.fragments() {
            let hit = match fragment {
                StreamingLayoutFragment::Text(fragment) => fragment
                    .closest_logical_offset_for_position(position)
                    .ok()?,
                StreamingLayoutFragment::OversizeAtom(fragment) => fragment
                    .closest_logical_offset_for_position(position)
                    .ok()?,
            };
            match hit {
                StreamingLayoutHit::Offset(offset) => return Some(ByteOffset::new(offset)),
                StreamingLayoutHit::BeforeFragment => return last.or(Some(self.viewport.start())),
                StreamingLayoutHit::AfterFragment => {
                    last = Some(match fragment {
                        StreamingLayoutFragment::Text(fragment) => {
                            ByteOffset::new(fragment.logical_range().end)
                        }
                        StreamingLayoutFragment::OversizeAtom(fragment) => {
                            ByteOffset::new(fragment.logical_range.end)
                        }
                    });
                }
            }
        }
        last
    }

    pub fn caret_bounds(&self, line_height: Pixels) -> Option<Bounds<Pixels>> {
        let _ = line_height;
        self.caret_geometry
    }

    pub(super) fn selection_bounds(
        &self,
        line_height: Pixels,
        wrap_width: Pixels,
    ) -> Vec<Bounds<Pixels>> {
        let _ = (line_height, wrap_width);
        self.selection_geometry.to_vec()
    }

    pub(super) fn bounds_for_range(
        &self,
        selected: ByteRange,
        line_height: Pixels,
        wrap_width: Pixels,
    ) -> Vec<Bounds<Pixels>> {
        if self.composition == Some(selected) {
            return self.composition_geometry.to_vec();
        }
        bounds_for_fragment_maps(self.fragments(), selected, line_height, wrap_width)
    }
}

fn bounds_for_fragment_maps(
    fragments: &[StreamingLayoutFragment],
    selected: ByteRange,
    line_height: Pixels,
    wrap_width: Pixels,
) -> Vec<Bounds<Pixels>> {
    if selected.is_empty() {
        return Vec::new();
    }
    let mut maps = Vec::new();
    for fragment in fragments {
        match fragment {
            StreamingLayoutFragment::Text(fragment) => maps.extend_from_slice(fragment.maps()),
            StreamingLayoutFragment::OversizeAtom(fragment) => {
                maps.extend_from_slice(fragment.maps())
            }
        }
    }
    maps.sort_by_key(|map| map.logical_offset);
    maps.dedup_by(|left, right| {
        left.logical_offset == right.logical_offset && left.position == right.position
    });
    let mut bounds = Vec::new();
    for pair in maps.windows(2) {
        let left = pair[0];
        let right = pair[1];
        if right.logical_offset <= selected.start().get()
            || left.logical_offset >= selected.end().get()
        {
            continue;
        }
        if left.position.y == right.position.y {
            bounds.push(Bounds::new(
                left.position,
                gpui::size(
                    (right.position.x - left.position.x).max(px(1.)),
                    line_height,
                ),
            ));
        } else {
            bounds.push(Bounds::new(
                left.position,
                gpui::size((wrap_width - left.position.x).max(px(1.)), line_height),
            ));
            bounds.push(Bounds::new(
                gpui::point(px(0.), right.position.y),
                gpui::size(right.position.x.max(px(1.)), line_height),
            ));
        }
    }
    bounds
}

fn position_for_fragments(
    fragments: &[StreamingLayoutFragment],
    offset: ByteOffset,
) -> Option<Point<Pixels>> {
    for fragment in fragments {
        match fragment {
            StreamingLayoutFragment::Text(fragment) => {
                let range = fragment.logical_range();
                if range.contains(&offset.get()) || offset.get() == range.end {
                    let local = usize::try_from(offset.get().checked_sub(range.start)?).ok()?;
                    if let Ok(Some(position)) = fragment.position_for_index(local) {
                        return Some(position);
                    }
                }
            }
            StreamingLayoutFragment::OversizeAtom(fragment) => {
                if let Some(position) = fragment.position_for_logical_offset(offset.get()) {
                    return Some(position);
                }
            }
        }
    }
    None
}

fn bounds_for_fragments(
    fragments: &[StreamingLayoutFragment],
    selected: ByteRange,
    line_height: Pixels,
    wrap_width: Pixels,
) -> Vec<Bounds<Pixels>> {
    bounds_for_fragment_maps(fragments, selected, line_height, wrap_width)
}
