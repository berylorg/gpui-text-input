use gpui::{
    Bounds, Pixels, Point, SharedString, StreamingBoundaryKind, StreamingLayoutFragment,
    StreamingLayoutHit, StreamingLayoutMap, StreamingLayoutPosition, StreamingObjectGap, px,
};

use crate::{
    AtomFact, BlockTargetPublication, ByteOffset, ByteRange, GeometryKey, GeometryQuality,
    InlineObjectId, InlineObjectOrder, ObjectPage, RangeBinding, RangePage, RangeSourceSelection,
    SourcePosition,
};

use super::{DesiredSurface, RangeSelection};
use super::{RangeRealizationCapacityState, RangeRealizationPriority};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangeSurfaceCharge {
    pub bytes: usize,
    pub items: usize,
}

#[derive(Clone, Copy, Debug)]
struct SurfacePageIndex {
    index: u32,
    start: ByteOffset,
}

impl RangeSurfaceCharge {
    pub fn replacement_peak(self, candidate: Self) -> Self {
        Self {
            bytes: self.bytes.saturating_add(candidate.bytes),
            items: self.items.saturating_add(candidate.items),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RealizedInlineObjectGeometry {
    id: InlineObjectId,
    order: InlineObjectOrder,
    leading: SourcePosition,
    trailing: SourcePosition,
    bounds: Bounds<Pixels>,
    hit_bounds: Bounds<Pixels>,
    leading_caret_bounds: Bounds<Pixels>,
    trailing_caret_bounds: Bounds<Pixels>,
    presentation_index: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct RealizedInlineObjectPresentation<'a> {
    geometry: RealizedInlineObjectGeometry,
    presentation: &'a crate::InlineObjectPresentation,
}

impl<'a> RealizedInlineObjectPresentation<'a> {
    pub const fn geometry(self) -> RealizedInlineObjectGeometry {
        self.geometry
    }

    pub const fn presentation(self) -> &'a crate::InlineObjectPresentation {
        self.presentation
    }
}

impl RealizedInlineObjectGeometry {
    pub const fn id(self) -> InlineObjectId {
        self.id
    }

    pub const fn order(self) -> InlineObjectOrder {
        self.order
    }

    pub const fn leading(self) -> SourcePosition {
        self.leading
    }

    pub const fn trailing(self) -> SourcePosition {
        self.trailing
    }

    pub const fn bounds(self) -> Bounds<Pixels> {
        self.bounds
    }

    pub const fn hit_bounds(self) -> Bounds<Pixels> {
        self.hit_bounds
    }

    pub const fn leading_caret_bounds(self) -> Bounds<Pixels> {
        self.leading_caret_bounds
    }

    pub const fn trailing_caret_bounds(self) -> Bounds<Pixels> {
        self.trailing_caret_bounds
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RealizedObjectGapGeometry {
    position: SourcePosition,
    caret_bounds: Bounds<Pixels>,
}

impl RealizedObjectGapGeometry {
    pub const fn position(self) -> SourcePosition {
        self.position
    }

    pub const fn caret_bounds(self) -> Bounds<Pixels> {
        self.caret_bounds
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeSurfaceHit {
    Gap(SourcePosition),
    Object(RealizedInlineObjectGeometry),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeSurfaceFiller {
    block_start: Pixels,
    block_end: Pixels,
    successor_block: Pixels,
}

impl RangeSurfaceFiller {
    pub const fn block_start(self) -> Pixels {
        self.block_start
    }

    pub const fn block_end(self) -> Pixels {
        self.block_end
    }

    pub const fn successor_block(self) -> Pixels {
        self.successor_block
    }

    pub fn contains(self, block: Pixels) -> bool {
        block >= self.block_start && block < self.block_end
    }
}

#[derive(Debug)]
pub struct CoherentRangeSurface {
    binding: RangeBinding,
    geometry: GeometryKey,
    pages: Vec<RangePage>,
    page_order: Box<[SurfacePageIndex]>,
    object_pages: Vec<ObjectPage>,
    selection: RangeSourceSelection,
    composition: Option<ByteRange>,
    scroll_position: SourcePosition,
    scroll_source: ByteOffset,
    scroll_block: Pixels,
    scroll_intra_anchor: Pixels,
    viewport: ByteRange,
    overscan: ByteRange,
    target: BlockTargetPublication,
    quality: GeometryQuality,
    visual_lines: u64,
    content_height: Pixels,
    line_height: Pixels,
    realized_objects: Box<[RealizedInlineObjectGeometry]>,
    realized_object_gaps: Box<[RealizedObjectGapGeometry]>,
    selection_geometry: Box<[Bounds<Pixels>]>,
    composition_geometry: Box<[Bounds<Pixels>]>,
    caret_geometry: Option<Bounds<Pixels>>,
    placeholder: Option<SharedString>,
    priority: RangeRealizationPriority,
    capacity_state: RangeRealizationCapacityState,
    fillers: [Option<RangeSurfaceFiller>; 2],
    charge: RangeSurfaceCharge,
}

pub(super) struct PreparedCoherentRangeSurface {
    binding: RangeBinding,
    geometry: GeometryKey,
    selection: RangeSourceSelection,
    page_order: Box<[SurfacePageIndex]>,
    composition: Option<ByteRange>,
    scroll_position: SourcePosition,
    scroll_source: ByteOffset,
    scroll_block: Pixels,
    scroll_intra_anchor: Pixels,
    viewport: ByteRange,
    overscan: ByteRange,
    quality: GeometryQuality,
    visual_lines: u64,
    content_height: Pixels,
    line_height: Pixels,
    realized_objects: Box<[RealizedInlineObjectGeometry]>,
    realized_object_gaps: Box<[RealizedObjectGapGeometry]>,
    selection_geometry: Box<[Bounds<Pixels>]>,
    composition_geometry: Box<[Bounds<Pixels>]>,
    caret_geometry: Option<Bounds<Pixels>>,
    placeholder: Option<SharedString>,
    priority: RangeRealizationPriority,
    capacity_state: RangeRealizationCapacityState,
    fillers: [Option<RangeSurfaceFiller>; 2],
    charge: RangeSurfaceCharge,
    candidate_charge: RangeSurfaceCharge,
}

impl PreparedCoherentRangeSurface {
    pub(super) const fn binding(&self) -> RangeBinding {
        self.binding
    }

    pub(super) const fn geometry_key(&self) -> GeometryKey {
        self.geometry
    }

    pub(super) const fn selection(&self) -> RangeSourceSelection {
        self.selection
    }

    pub(super) const fn scroll_source(&self) -> ByteOffset {
        self.scroll_source
    }

    pub(super) const fn scroll_intra_anchor(&self) -> Pixels {
        self.scroll_intra_anchor
    }

    pub(super) const fn charge(&self) -> RangeSurfaceCharge {
        self.charge
    }

    pub(super) const fn candidate_charge(&self) -> RangeSurfaceCharge {
        self.candidate_charge
    }

    pub(super) fn object_selected_by(
        &self,
        selection: RangeSourceSelection,
    ) -> Option<RealizedInlineObjectGeometry> {
        object_selected_by(&self.realized_objects, selection)
    }
}

impl CoherentRangeSurface {
    pub(super) fn prepare<'a>(
        binding: RangeBinding,
        pages: impl ExactSizeIterator<Item = &'a RangePage> + Clone,
        object_pages: impl ExactSizeIterator<Item = &'a ObjectPage> + Clone,
        desired: DesiredSurface,
        restored_positions: Option<(SourcePosition, RangeSourceSelection)>,
        preserved_scroll_position: Option<SourcePosition>,
        target: &BlockTargetPublication,
        quality: GeometryQuality,
        visual_lines: u64,
        content_height: Pixels,
        line_height: Pixels,
        wrap_width: Pixels,
        placeholder: SharedString,
    ) -> Result<PreparedCoherentRangeSurface, crate::RangeTextInputError> {
        let viewport = ByteRange::new(
            target.target_source().byte_offset,
            target.source_end().byte_offset,
        )?;
        let overscan = ByteRange::new(
            target.predecessor().byte_offset,
            target.source_end().byte_offset,
        )?;
        let mut page_order = Vec::with_capacity(pages.len());
        for (index, page) in pages.clone().enumerate() {
            page_order.push(SurfacePageIndex {
                index: u32::try_from(index)
                    .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?,
                start: page.range().start(),
            });
        }
        page_order.sort_by_key(|entry| entry.start);
        let page_order_candidate_bytes = page_order
            .capacity()
            .checked_mul(std::mem::size_of::<SurfacePageIndex>())
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let page_order_candidate_items = page_order.capacity();
        let page_order_bytes = page_order
            .len()
            .checked_mul(std::mem::size_of::<SurfacePageIndex>())
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let page_order_items = page_order.len();
        let page_bytes = pages
            .clone()
            .try_fold(0usize, |total, page| {
                total.checked_add(page.retained_charge().bytes())
            })
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let page_items = pages
            .clone()
            .try_fold(0usize, |total, page| {
                total.checked_add(page.retained_charge().items())
            })
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let object_page_bytes = object_pages
            .clone()
            .try_fold(0usize, |total, page| {
                total.checked_add(page.retained_charge().bytes())
            })
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let object_page_items = object_pages
            .clone()
            .try_fold(0usize, |total, page| {
                total.checked_add(page.retained_charge().allocated_items().checked_add(1)?)
            })
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let presentation_overlap = target
            .presentation_overlap_bytes(object_pages.clone())
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let fragment_bytes = target
            .charge()
            .total()
            .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?
            .checked_add(
                target
                    .output_record_bytes()
                    .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
            )
            .and_then(|bytes| bytes.checked_sub(presentation_overlap))
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let fragment_items = target
            .item_charge()
            .total()
            .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?
            .checked_add(target.object_presentation_items())
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        for page in object_pages.clone() {
            let key = page.key();
            if key.binding() != target.key().geometry().binding()
                || key.revision() != target.key().geometry().revision()
                || key.presentation_generation()
                    != target.key().geometry().presentation_generation()
            {
                return Err(crate::RangeTextInputError::IncompleteSurface);
            }
        }
        let owned_maps = collect_owned_fragment_maps(target.fragments())?;
        let temporary_map_bytes = owned_maps
            .capacity()
            .checked_mul(std::mem::size_of::<StreamingLayoutMap>())
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let temporary_map_items = owned_maps.capacity();
        let (realized_objects, realized_object_gaps) = realize_composite_geometry(
            target.object_presentations(),
            target.fragments(),
            line_height,
            &owned_maps,
        )?;
        let realized_candidate_items = realized_objects
            .capacity()
            .checked_add(realized_object_gaps.capacity())
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let realized_candidate_bytes = realized_objects
            .capacity()
            .checked_mul(std::mem::size_of::<RealizedInlineObjectGeometry>())
            .and_then(|value| {
                realized_object_gaps
                    .capacity()
                    .checked_mul(std::mem::size_of::<RealizedObjectGapGeometry>())
                    .and_then(|gaps| value.checked_add(gaps))
            })
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let realized_items = realized_objects
            .len()
            .checked_add(realized_object_gaps.len())
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let realized_bytes = realized_objects
            .len()
            .checked_mul(std::mem::size_of::<RealizedInlineObjectGeometry>())
            .and_then(|value| {
                realized_object_gaps
                    .len()
                    .checked_mul(std::mem::size_of::<RealizedObjectGapGeometry>())
                    .and_then(|gaps| value.checked_add(gaps))
            })
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let selection = restored_positions
            .map(|(_, selection)| selection)
            .or(desired.source_selection)
            .unwrap_or_else(|| RangeSourceSelection::caret(target.target_source()));
        let selection_geometry = bounds_for_composite_selection_from_maps(
            &owned_maps,
            &realized_objects,
            selection,
            line_height,
            wrap_width,
        );
        let composition_geometry = desired.composition.map_or_else(Vec::new, |range| {
            bounds_for_owned_fragment_maps(&owned_maps, range, line_height, wrap_width)
        });
        let caret_geometry = position_for_composite_fragments(target.fragments(), selection.head)
            .map(|origin| Bounds::new(origin, gpui::size(px(2.), line_height)));
        let geometry_candidate_items = selection_geometry
            .capacity()
            .checked_add(composition_geometry.capacity())
            .and_then(|value| value.checked_add(usize::from(caret_geometry.is_some())))
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let geometry_candidate_bytes = selection_geometry
            .capacity()
            .checked_add(composition_geometry.capacity())
            .and_then(|value| value.checked_add(usize::from(caret_geometry.is_some())))
            .and_then(|items| items.checked_mul(std::mem::size_of::<Bounds<Pixels>>()))
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
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
        let placeholder_bytes = placeholder
            .as_ref()
            .map_or(Some(0), |placeholder| {
                std::mem::size_of::<SharedString>().checked_add(placeholder.len())
            })
            .ok_or(crate::RangeTextInputError::SurfaceCapacity)?;
        let placeholder_items = usize::from(placeholder.is_some());
        let charge = RangeSurfaceCharge {
            bytes: std::mem::size_of::<Self>()
                .checked_add(page_bytes)
                .and_then(|value| value.checked_add(page_order_bytes))
                .and_then(|value| value.checked_add(object_page_bytes))
                .and_then(|value| value.checked_add(fragment_bytes))
                .and_then(|value| value.checked_add(realized_bytes))
                .and_then(|value| value.checked_add(geometry_bytes))
                .and_then(|value| value.checked_add(placeholder_bytes))
                .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
            items: 1usize
                .checked_add(page_items)
                .and_then(|value| value.checked_add(page_order_items))
                .and_then(|value| value.checked_add(object_page_items))
                .and_then(|value| value.checked_add(fragment_items))
                .and_then(|value| value.checked_add(realized_items))
                .and_then(|value| value.checked_add(geometry_items))
                .and_then(|value| value.checked_add(placeholder_items))
                .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
        };
        let candidate_charge = RangeSurfaceCharge {
            bytes: page_order_candidate_bytes
                .checked_add(realized_candidate_bytes)
                .and_then(|value| value.checked_add(geometry_candidate_bytes))
                .and_then(|value| value.checked_add(temporary_map_bytes))
                .and_then(|value| value.checked_add(placeholder_bytes))
                .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
            items: page_order_candidate_items
                .checked_add(realized_candidate_items)
                .and_then(|value| value.checked_add(geometry_candidate_items))
                .and_then(|value| value.checked_add(temporary_map_items))
                .and_then(|value| value.checked_add(placeholder_items))
                .ok_or(crate::RangeTextInputError::SurfaceCapacity)?,
        };
        let scroll_position = if desired.preserve_scroll_anchor {
            preserved_scroll_position.unwrap_or_else(|| target.target_source())
        } else {
            target.target_source()
        };
        let scroll_source = scroll_position.byte_offset;
        let ordinary_anchor_block =
            matches!(scroll_position.gap, crate::InlineObjectGap::NoObjects)
                .then(|| {
                    target
                        .fragments()
                        .iter()
                        .find_map(|fragment| match fragment {
                            StreamingLayoutFragment::Text(fragment) => {
                                let range = fragment.logical_range();
                                if scroll_source.get() >= range.start.byte_offset
                                    && scroll_source.get() <= range.end.byte_offset
                                {
                                    fragment
                                        .position_for_logical_position(StreamingLayoutPosition::at(
                                            scroll_source.get(),
                                        ))
                                        .ok()
                                        .flatten()
                                        .map(|position| position.y)
                                } else {
                                    None
                                }
                            }
                            StreamingLayoutFragment::OversizeAtom(fragment) => fragment
                                .position_for_logical_position(StreamingLayoutPosition::at(
                                    scroll_source.get(),
                                ))
                                .map(|position| position.y),
                            StreamingLayoutFragment::Boundary(fragment) => fragment
                                .position_for_logical_position(StreamingLayoutPosition::at(
                                    scroll_source.get(),
                                ))
                                .map(|position| position.y),
                            StreamingLayoutFragment::InlineObject(_) => None,
                        })
                })
                .flatten();
        let explicit_scroll_block = (!desired.preserve_scroll_anchor
            && matches!(
                desired.priority(),
                RangeRealizationPriority::ScrollAnchor | RangeRealizationPriority::NearbyContent
            ))
        .then_some(desired.target_block);
        let anchor_block = explicit_scroll_block
            .or(ordinary_anchor_block)
            .or_else(|| {
                position_for_composite_fragments(target.fragments(), scroll_position)
                    .map(|position| position.y)
            })
            .or_else(|| (!desired.preserve_scroll_anchor).then_some(desired.target_block))
            .or_else(|| {
                // A terminal-complete target at the exact source end owns no fragment window.
                // Its caret is on the final visual line, one line height before total content.
                (target.fragments().is_empty()
                    && scroll_source.get() == binding.extent().byte_len())
                .then(|| (content_height - line_height).max(Pixels::ZERO))
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
        let max_scroll = (content_height - desired.viewport_extent).max(Pixels::ZERO);
        let scroll_block = (anchor_block + scroll_intra_anchor)
            .max(Pixels::ZERO)
            .min(max_scroll);
        let scroll_intra_anchor = (scroll_block - anchor_block).max(Pixels::ZERO);
        let fillers = filler_for_exact_surface(desired, scroll_block, content_height);
        let has_filler = fillers.iter().any(Option::is_some);
        let capacity_state = match (desired.capacity_saturated, has_filler) {
            (false, false) => RangeRealizationCapacityState::Normal,
            (true, false) => RangeRealizationCapacityState::CapacitySaturated,
            (false, true) => RangeRealizationCapacityState::ViewportExceedsRenderingCapacity,
            (true, true) => {
                RangeRealizationCapacityState::CapacitySaturatedViewportExceedsRenderingCapacity
            }
        };
        Ok(PreparedCoherentRangeSurface {
            binding,
            geometry: target.key().geometry(),
            selection,
            page_order: page_order.into_boxed_slice(),
            composition: desired.composition,
            scroll_position,
            scroll_source,
            scroll_block,
            scroll_intra_anchor,
            viewport,
            overscan,
            quality,
            visual_lines,
            content_height,
            line_height,
            realized_objects: realized_objects.into_boxed_slice(),
            realized_object_gaps: realized_object_gaps.into_boxed_slice(),
            selection_geometry: selection_geometry.into_boxed_slice(),
            composition_geometry: composition_geometry.into_boxed_slice(),
            caret_geometry,
            placeholder,
            priority: desired.priority(),
            capacity_state,
            fillers,
            charge,
            candidate_charge,
        })
    }

    pub(super) fn commit_prepared(
        prepared: PreparedCoherentRangeSurface,
        pages: Vec<RangePage>,
        object_pages: Vec<ObjectPage>,
        target: BlockTargetPublication,
    ) -> Self {
        debug_assert_eq!(prepared.geometry, target.key().geometry());
        Self {
            binding: prepared.binding,
            geometry: prepared.geometry,
            pages,
            page_order: prepared.page_order,
            object_pages,
            selection: prepared.selection,
            composition: prepared.composition,
            scroll_position: prepared.scroll_position,
            scroll_source: prepared.scroll_source,
            scroll_block: prepared.scroll_block,
            scroll_intra_anchor: prepared.scroll_intra_anchor,
            viewport: prepared.viewport,
            overscan: prepared.overscan,
            target,
            quality: prepared.quality,
            visual_lines: prepared.visual_lines,
            content_height: prepared.content_height,
            line_height: prepared.line_height,
            realized_objects: prepared.realized_objects,
            realized_object_gaps: prepared.realized_object_gaps,
            selection_geometry: prepared.selection_geometry,
            composition_geometry: prepared.composition_geometry,
            caret_geometry: prepared.caret_geometry,
            placeholder: prepared.placeholder,
            priority: prepared.priority,
            capacity_state: prepared.capacity_state,
            fillers: prepared.fillers,
            charge: prepared.charge,
        }
    }

    pub const fn binding(&self) -> RangeBinding {
        self.binding
    }
    pub const fn geometry_key(&self) -> GeometryKey {
        self.geometry
    }
    pub const fn selection(&self) -> RangeSourceSelection {
        self.selection
    }
    pub const fn source_caret(&self) -> SourcePosition {
        self.selection.head
    }
    pub const fn source_selection(&self) -> RangeSourceSelection {
        self.selection
    }
    pub const fn composition(&self) -> Option<ByteRange> {
        self.composition
    }
    pub const fn caret(&self) -> SourcePosition {
        self.selection.head
    }
    pub fn platform_selection(&self) -> Option<RangeSelection> {
        matches!(self.selection.anchor.gap, crate::InlineObjectGap::NoObjects).then_some(())?;
        matches!(self.selection.head.gap, crate::InlineObjectGap::NoObjects).then_some(())?;
        Some(RangeSelection {
            anchor: self.selection.anchor.byte_offset,
            head: self.selection.head.byte_offset,
        })
    }
    pub const fn scroll_source(&self) -> ByteOffset {
        self.scroll_source
    }
    pub const fn scroll_position(&self) -> SourcePosition {
        self.scroll_position
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

    pub(super) fn local_checkpoint_for(
        &self,
        _block: Pixels,
        _anchor: Option<SourcePosition>,
    ) -> Option<&crate::ExactGeometryCheckpoint> {
        if self.geometry != self.target.key().geometry() {
            return None;
        }
        Some(self.target.predecessor_checkpoint())
    }
    pub const fn charge(&self) -> RangeSurfaceCharge {
        self.charge
    }
    pub const fn realization_priority(&self) -> RangeRealizationPriority {
        self.priority
    }
    pub const fn capacity_state(&self) -> RangeRealizationCapacityState {
        self.capacity_state
    }
    pub fn filler_count(&self) -> usize {
        self.fillers
            .iter()
            .filter(|filler| filler.is_some())
            .count()
    }
    pub const fn filler(&self) -> Option<RangeSurfaceFiller> {
        match self.fillers[1] {
            Some(filler) => Some(filler),
            None => self.fillers[0],
        }
    }
    pub fn fillers(&self) -> impl Iterator<Item = RangeSurfaceFiller> + '_ {
        self.fillers.iter().flatten().copied()
    }
    pub fn filler_at(&self, block: Pixels) -> Option<RangeSurfaceFiller> {
        self.fillers().find(|filler| filler.contains(block))
    }
    pub fn pages(&self) -> &[RangePage] {
        &self.pages
    }

    pub(super) fn pages_in_source_order(&self) -> impl ExactSizeIterator<Item = &RangePage> {
        self.page_order
            .iter()
            .map(|entry| &self.pages[entry.index as usize])
    }
    pub fn object_pages(&self) -> &[ObjectPage] {
        &self.object_pages
    }
    pub fn realized_objects(&self) -> &[RealizedInlineObjectGeometry] {
        &self.realized_objects
    }
    pub fn realized_presentations(
        &self,
        publication: crate::GeometryJobKey,
    ) -> Option<impl ExactSizeIterator<Item = RealizedInlineObjectPresentation<'_>>> {
        (publication == self.target.key()).then(|| {
            self.realized_objects
                .iter()
                .copied()
                .map(|geometry| RealizedInlineObjectPresentation {
                    presentation: self.presentation_for_geometry(geometry),
                    geometry,
                })
        })
    }

    pub const fn publication_key(&self) -> crate::GeometryJobKey {
        self.target.key()
    }

    pub(super) fn presentation_for_geometry(
        &self,
        geometry: RealizedInlineObjectGeometry,
    ) -> &crate::InlineObjectPresentation {
        self.target.object_presentations()[geometry.presentation_index as usize].presentation()
    }
    pub fn realized_object_gaps(&self) -> &[RealizedObjectGapGeometry] {
        &self.realized_object_gaps
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
                    if offset >= range.start.byte_offset && offset <= range.end.byte_offset {
                        if let Ok(Some(position)) = fragment
                            .position_for_logical_position(StreamingLayoutPosition::at(offset))
                        {
                            return Some(position);
                        }
                    }
                }
                StreamingLayoutFragment::OversizeAtom(fragment) => {
                    if let Some(position) =
                        fragment.position_for_logical_position(StreamingLayoutPosition::at(offset))
                    {
                        return Some(position);
                    }
                }
                StreamingLayoutFragment::Boundary(fragment) => {
                    if let Some(position) =
                        fragment.position_for_logical_position(StreamingLayoutPosition::at(offset))
                    {
                        return Some(position);
                    }
                }
                StreamingLayoutFragment::InlineObject(_) => {}
            }
        }
        None
    }

    pub fn position_for_source_position(&self, position: SourcePosition) -> Option<Point<Pixels>> {
        if position == self.selection.head {
            return self.caret_geometry.map(|bounds| bounds.origin);
        }
        self.realized_object_gaps
            .binary_search_by(|gap| {
                gap.position
                    .compare_in_revision(position)
                    .unwrap_or(std::cmp::Ordering::Less)
            })
            .ok()
            .and_then(|index| self.realized_object_gaps.get(index))
            .map(|gap| gap.caret_bounds.origin)
            .or_else(|| {
                let position = position.into();
                self.fragments().iter().find_map(|fragment| match fragment {
                    StreamingLayoutFragment::Text(fragment) => fragment
                        .position_for_logical_position(position)
                        .ok()
                        .flatten(),
                    StreamingLayoutFragment::OversizeAtom(fragment) => {
                        fragment.position_for_logical_position(position)
                    }
                    StreamingLayoutFragment::InlineObject(fragment) => {
                        fragment.position_for_logical_position(position)
                    }
                    StreamingLayoutFragment::Boundary(fragment) => {
                        fragment.position_for_logical_position(position)
                    }
                })
            })
    }

    pub(super) fn adjacent_object(
        &self,
        position: SourcePosition,
        direction: crate::SegmentationDirection,
    ) -> Option<RealizedInlineObjectGeometry> {
        let compare = |candidate: SourcePosition| {
            candidate
                .compare_in_revision(position)
                .unwrap_or(std::cmp::Ordering::Less)
        };
        let index = match direction {
            crate::SegmentationDirection::Forward => self
                .realized_objects
                .binary_search_by(|object| compare(object.leading))
                .ok()?,
            crate::SegmentationDirection::Reverse => self
                .realized_objects
                .binary_search_by(|object| compare(object.trailing))
                .ok()?,
        };
        self.realized_objects.get(index).copied()
    }

    pub(super) fn object_selected_by(
        &self,
        selection: RangeSourceSelection,
    ) -> Option<RealizedInlineObjectGeometry> {
        object_selected_by(&self.realized_objects, selection)
    }

    pub(super) fn source_position_for_byte(
        &self,
        offset: ByteOffset,
        direction: crate::SegmentationDirection,
    ) -> Option<SourcePosition> {
        source_position_for_byte(
            &self.realized_object_gaps,
            self.fragments(),
            offset,
            direction,
        )
    }

    pub fn hit_test_composite(&self, position: Point<Pixels>) -> Option<RangeSurfaceHit> {
        if self.filler_at(position.y).is_some() {
            return None;
        }
        let mut realized_index = 0usize;
        self.fragments().iter().find_map(|fragment| {
            let current_object = matches!(fragment, StreamingLayoutFragment::InlineObject(_))
                .then(|| {
                    let object = self.realized_objects.get(realized_index).copied();
                    realized_index += 1;
                    object
                })
                .flatten();
            let hit = match fragment {
                StreamingLayoutFragment::Text(fragment) => fragment
                    .closest_logical_position_for_position(position)
                    .ok()?,
                StreamingLayoutFragment::OversizeAtom(fragment) => fragment
                    .closest_logical_position_for_position(position)
                    .ok()?,
                StreamingLayoutFragment::InlineObject(fragment) => fragment
                    .closest_logical_position_for_position(position)
                    .ok()?,
                StreamingLayoutFragment::Boundary(fragment) => fragment
                    .closest_logical_position_for_position(position)
                    .ok()?,
            }?;
            match hit {
                StreamingLayoutHit::Gap(position) => SourcePosition::try_from(position)
                    .ok()
                    .map(RangeSurfaceHit::Gap),
                StreamingLayoutHit::Object(id) => current_object
                    .filter(|object| object.id == InlineObjectId::from(id))
                    .map(RangeSurfaceHit::Object),
            }
        })
    }

    pub fn hit_test(&self, position: Point<Pixels>) -> Option<ByteOffset> {
        let mut leading = None;
        let mut trailing = None;
        let mut exact_viewport_start = false;
        for fragment in self.fragments() {
            let (hit, leading_edge, trailing_edge, edge_block_extent) = match fragment {
                StreamingLayoutFragment::Text(fragment) => (
                    fragment
                        .closest_logical_position_for_position(position)
                        .ok()?,
                    fragment.maps().first(),
                    fragment.maps().last(),
                    self.line_height,
                ),
                StreamingLayoutFragment::OversizeAtom(fragment) => (
                    fragment
                        .closest_logical_position_for_position(position)
                        .ok()?,
                    fragment.maps().first(),
                    fragment.maps().last(),
                    fragment.bounds.size.height,
                ),
                StreamingLayoutFragment::Boundary(fragment) => {
                    let maps = fragment.maps();
                    let (leading_edge, trailing_edge) = match fragment.kind {
                        StreamingBoundaryKind::LogicalLine => (
                            maps.last(),
                            (maps.len() > 1).then(|| maps.first()).flatten(),
                        ),
                        StreamingBoundaryKind::EndOfSource => (None, maps.last()),
                    };
                    (
                        fragment
                            .closest_logical_position_for_position(position)
                            .ok()?,
                        leading_edge,
                        trailing_edge,
                        self.line_height,
                    )
                }
                StreamingLayoutFragment::InlineObject(fragment) => {
                    if fragment
                        .closest_logical_position_for_position(position)
                        .ok()?
                        .is_some()
                    {
                        return None;
                    }
                    continue;
                }
            };
            match hit {
                Some(StreamingLayoutHit::Gap(position)) => {
                    return ordinary_offset(position).map(ByteOffset::new);
                }
                Some(StreamingLayoutHit::Object(_)) => return None,
                None => {}
            }
            record_ordinary_edge_fallback(
                leading_edge,
                trailing_edge,
                position,
                edge_block_extent,
                self.viewport.start(),
                &mut leading,
                &mut trailing,
                &mut exact_viewport_start,
            );
        }
        leading
            .and_then(ordinary_offset)
            .and_then(|offset| {
                (offset >= self.viewport.start().get())
                    .then_some(ByteOffset::new(offset))
                    .or_else(|| exact_viewport_start.then_some(self.viewport.start()))
            })
            .or_else(|| trailing.and_then(ordinary_offset).map(ByteOffset::new))
    }

    pub fn caret_bounds(&self, line_height: Pixels) -> Option<Bounds<Pixels>> {
        let _ = line_height;
        self.caret_geometry
    }

    pub(super) fn selection_bounds(&self) -> &[Bounds<Pixels>] {
        &self.selection_geometry
    }

    pub(super) fn composition_bounds(&self) -> &[Bounds<Pixels>] {
        &self.composition_geometry
    }

    pub(super) fn first_last_bounds_for_range(
        &self,
        selected: ByteRange,
        line_height: Pixels,
        wrap_width: Pixels,
    ) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        if self.composition == Some(selected) {
            return self
                .composition_geometry
                .first()
                .copied()
                .zip(self.composition_geometry.last().copied());
        }
        first_last_bounds_for_fragment_maps(self.fragments(), selected, line_height, wrap_width)
    }
}

fn filler_for_exact_surface(
    desired: DesiredSurface,
    scroll_block: Pixels,
    content_height: Pixels,
) -> [Option<RangeSurfaceFiller>; 2] {
    let exact_remaining = (content_height - scroll_block).max(Pixels::ZERO);
    let exact_visible = exact_remaining.min(desired.viewport_extent);
    let visible_end = scroll_block + exact_visible;
    let realized_start = desired
        .realization_anchor_block
        .clamp(scroll_block, visible_end);
    let realized_end = (realized_start + desired.realization_extent).min(visible_end);
    let leading = (realized_start > scroll_block).then(|| RangeSurfaceFiller {
        block_start: scroll_block,
        block_end: realized_start,
        successor_block: (realized_start - desired.realization_extent).max(scroll_block),
    });
    let trailing = (visible_end > realized_end).then(|| RangeSurfaceFiller {
        block_start: realized_end,
        block_end: scroll_block + exact_visible,
        successor_block: realized_end.min(content_height),
    });
    [leading, trailing]
}

fn source_position_for_byte(
    gaps: &[RealizedObjectGapGeometry],
    fragments: &[StreamingLayoutFragment],
    offset: ByteOffset,
    direction: crate::SegmentationDirection,
) -> Option<SourcePosition> {
    let start = gaps.partition_point(|gap| gap.position.byte_offset < offset);
    let end = gaps.partition_point(|gap| gap.position.byte_offset <= offset);
    if start != end {
        return match direction {
            crate::SegmentationDirection::Forward => gaps.get(start).map(|gap| gap.position),
            crate::SegmentationDirection::Reverse => gaps.get(end - 1).map(|gap| gap.position),
        };
    }
    let gpui_position = StreamingLayoutPosition::at(offset.get());
    fragments.iter().find_map(|fragment| {
        let mapped = match fragment {
            StreamingLayoutFragment::Text(fragment) => fragment
                .position_for_logical_position(gpui_position)
                .ok()
                .flatten(),
            StreamingLayoutFragment::OversizeAtom(fragment) => {
                fragment.position_for_logical_position(gpui_position)
            }
            StreamingLayoutFragment::InlineObject(fragment) => {
                fragment.position_for_logical_position(gpui_position)
            }
            StreamingLayoutFragment::Boundary(fragment) => {
                fragment.position_for_logical_position(gpui_position)
            }
        }?;
        let _ = mapped;
        Some(SourcePosition::new(
            offset,
            crate::InlineObjectGap::NoObjects,
        ))
    })
}

fn object_selected_by(
    realized_objects: &[RealizedInlineObjectGeometry],
    selection: RangeSourceSelection,
) -> Option<RealizedInlineObjectGeometry> {
    let selected = selection.range().ok()?;
    let index = realized_objects
        .binary_search_by(|object| {
            object
                .leading
                .compare_in_revision(selected.start())
                .unwrap_or(std::cmp::Ordering::Less)
        })
        .ok()?;
    realized_objects
        .get(index)
        .copied()
        .filter(|object| object.trailing == selected.end())
}

fn realize_composite_geometry<'a>(
    object_presentations: &'a [crate::range_geometry::TargetInlineObjectPresentation],
    fragments: &[StreamingLayoutFragment],
    line_height: Pixels,
    owned_maps: &[StreamingLayoutMap],
) -> Result<
    (
        Vec<RealizedInlineObjectGeometry>,
        Vec<RealizedObjectGapGeometry>,
    ),
    crate::RangeTextInputError,
> {
    let object_count = fragments
        .iter()
        .filter(|fragment| matches!(fragment, StreamingLayoutFragment::InlineObject(_)))
        .count();
    if object_count != object_presentations.len() {
        return Err(crate::RangeTextInputError::IncompleteSurface);
    }
    let gap_count = owned_maps
        .iter()
        .filter(|map| map.logical_position.gap != StreamingObjectGap::no_objects())
        .count();
    let mut objects = Vec::with_capacity(object_count);
    let mut gaps: Vec<RealizedObjectGapGeometry> = Vec::with_capacity(gap_count);
    let mut previous = None;
    for map in owned_maps {
        if map.logical_position.gap == StreamingObjectGap::no_objects() {
            continue;
        }
        let position = SourcePosition::try_from(map.logical_position)
            .map_err(|_| crate::RangeTextInputError::IncompleteSurface)?;
        if previous.is_some_and(|previous: SourcePosition| {
            previous
                .compare_in_revision(position)
                .is_none_or(|ordering| !ordering.is_lt())
        }) {
            return Err(crate::RangeTextInputError::IncompleteSurface);
        }
        previous = Some(position);
        gaps.push(RealizedObjectGapGeometry {
            position,
            caret_bounds: Bounds::new(map.position, gpui::size(px(2.), line_height)),
        });
    }
    let mut presentation_index = 0usize;
    for fragment in fragments {
        let StreamingLayoutFragment::InlineObject(fragment) = fragment else {
            continue;
        };
        let id = InlineObjectId::from(fragment.id);
        let order = InlineObjectOrder::from(fragment.order);
        let leading = SourcePosition::try_from(fragment.leading)
            .map_err(|_| crate::RangeTextInputError::IncompleteSurface)?;
        let trailing = SourcePosition::try_from(fragment.trailing)
            .map_err(|_| crate::RangeTextInputError::IncompleteSurface)?;
        if leading.byte_offset != trailing.byte_offset {
            return Err(crate::RangeTextInputError::IncompleteSurface);
        }
        let Some(record) = object_presentations.get(presentation_index) else {
            return Err(crate::RangeTextInputError::IncompleteSurface);
        };
        let cursor = record.cursor();
        if (cursor.anchor(), cursor.order(), cursor.id()) != (leading.byte_offset, order, id) {
            return Err(crate::RangeTextInputError::IncompleteSurface);
        }
        let presentation = record.presentation();
        if presentation.display() != fragment.presentation.as_ref()
            || presentation.width() != fragment.bounds.size.width
            || presentation.height() != fragment.bounds.size.height
            || presentation.baseline() != fragment.baseline()
        {
            return Err(crate::RangeTextInputError::IncompleteSurface);
        }
        let leading_caret_bounds =
            gap_bounds(&gaps, leading).ok_or(crate::RangeTextInputError::IncompleteSurface)?;
        let trailing_caret_bounds =
            gap_bounds(&gaps, trailing).ok_or(crate::RangeTextInputError::IncompleteSurface)?;
        objects.push(RealizedInlineObjectGeometry {
            id,
            order,
            leading,
            trailing,
            bounds: fragment.bounds,
            hit_bounds: fragment.bounds,
            leading_caret_bounds,
            trailing_caret_bounds,
            presentation_index: u32::try_from(presentation_index)
                .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?,
        });
        presentation_index += 1;
    }
    Ok((objects, gaps))
}

fn gap_bounds(
    gaps: &[RealizedObjectGapGeometry],
    position: SourcePosition,
) -> Option<Bounds<Pixels>> {
    gaps.binary_search_by(|gap| {
        gap.position
            .compare_in_revision(position)
            .unwrap_or(std::cmp::Ordering::Less)
    })
    .ok()
    .and_then(|index| gaps.get(index))
    .map(|gap| gap.caret_bounds)
}

fn collect_owned_fragment_maps(
    fragments: &[StreamingLayoutFragment],
) -> Result<Vec<StreamingLayoutMap>, crate::RangeTextInputError> {
    let mut count = 0usize;
    let mut previous = None;
    for fragment in fragments {
        for_each_owned_fragment_map(fragment, &mut |map| {
            if previous != Some(map.logical_position) {
                count = count.checked_add(1).unwrap_or(usize::MAX);
                previous = Some(map.logical_position);
            }
        });
    }
    if count == usize::MAX {
        return Err(crate::RangeTextInputError::SurfaceCapacity);
    }
    let mut maps = Vec::with_capacity(count);
    previous = None;
    for fragment in fragments {
        for_each_owned_fragment_map(fragment, &mut |map| {
            if previous != Some(map.logical_position) {
                maps.push(map);
                previous = Some(map.logical_position);
            }
        });
    }
    Ok(maps)
}

fn for_each_owned_fragment_map(
    fragment: &StreamingLayoutFragment,
    visit: &mut impl FnMut(StreamingLayoutMap),
) {
    match fragment {
        StreamingLayoutFragment::Text(fragment) => fragment
            .maps()
            .iter()
            .copied()
            .take(fragment.maps().len().saturating_sub(1))
            .for_each(visit),
        StreamingLayoutFragment::OversizeAtom(fragment) => {
            fragment.maps().first().copied().into_iter().for_each(visit)
        }
        StreamingLayoutFragment::InlineObject(fragment) => {
            fragment.maps().iter().copied().for_each(visit)
        }
        StreamingLayoutFragment::Boundary(fragment) => match fragment.kind {
            StreamingBoundaryKind::LogicalLine if fragment.maps().len() > 1 => {
                fragment.maps().first().copied().into_iter().for_each(visit)
            }
            StreamingBoundaryKind::EndOfSource => {
                fragment.maps().first().copied().into_iter().for_each(visit)
            }
            StreamingBoundaryKind::LogicalLine => {}
        },
    }
}

fn bounds_for_owned_fragment_maps(
    maps: &[StreamingLayoutMap],
    selected: ByteRange,
    line_height: Pixels,
    wrap_width: Pixels,
) -> Vec<Bounds<Pixels>> {
    if selected.is_empty() {
        return Vec::new();
    }
    let mut bounds = Vec::new();
    let mut previous = None;
    for right in maps
        .iter()
        .copied()
        .filter(|map| map.logical_position.gap == StreamingObjectGap::no_objects())
    {
        let Some(left) = previous.replace(right) else {
            continue;
        };
        if right.logical_position.byte_offset <= selected.start().get()
            || left.logical_position.byte_offset >= selected.end().get()
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

fn bounds_for_composite_selection_from_maps(
    maps: &[StreamingLayoutMap],
    objects: &[RealizedInlineObjectGeometry],
    selection: RangeSourceSelection,
    line_height: Pixels,
    wrap_width: Pixels,
) -> Vec<Bounds<Pixels>> {
    let Ok(selected) = selection.range() else {
        return Vec::new();
    };
    let Ok(byte_range) = ByteRange::new(selected.start().byte_offset, selected.end().byte_offset)
    else {
        return Vec::new();
    };
    let mut bounds = bounds_for_owned_fragment_maps(maps, byte_range, line_height, wrap_width);
    bounds.extend(
        objects
            .iter()
            .filter(|object| {
                selected
                    .start()
                    .compare_in_revision(object.leading)
                    .is_some_and(|ordering| !ordering.is_gt())
                    && object
                        .trailing
                        .compare_in_revision(selected.end())
                        .is_some_and(|ordering| !ordering.is_gt())
            })
            .map(|object| object.bounds),
    );
    bounds
}

fn first_last_bounds_for_fragment_maps(
    fragments: &[StreamingLayoutFragment],
    selected: ByteRange,
    line_height: Pixels,
    wrap_width: Pixels,
) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
    if selected.is_empty() {
        return None;
    }
    let mut previous = None;
    let mut first = None;
    let mut last = None;
    let mut ordered = true;
    let mut consider = |map: StreamingLayoutMap| {
        let Some(left) = previous else {
            previous = Some(map);
            return;
        };
        if map.logical_position.byte_offset < left.logical_position.byte_offset {
            ordered = false;
            return;
        }
        if map.logical_position == left.logical_position && map.position == left.position {
            return;
        }
        previous = Some(map);
        if map.logical_position.byte_offset <= selected.start().get()
            || left.logical_position.byte_offset >= selected.end().get()
        {
            return;
        }
        let mut record = |bounds| {
            first.get_or_insert(bounds);
            last = Some(bounds);
        };
        if left.position.y == map.position.y {
            record(Bounds::new(
                left.position,
                gpui::size((map.position.x - left.position.x).max(px(1.)), line_height),
            ));
        } else {
            record(Bounds::new(
                left.position,
                gpui::size((wrap_width - left.position.x).max(px(1.)), line_height),
            ));
            record(Bounds::new(
                gpui::point(px(0.), map.position.y),
                gpui::size(map.position.x.max(px(1.)), line_height),
            ));
        }
    };
    for fragment in fragments {
        match fragment {
            StreamingLayoutFragment::Text(fragment) => {
                for map in fragment.maps() {
                    if ordinary_offset(map.logical_position).is_some() {
                        consider(*map);
                    }
                }
            }
            StreamingLayoutFragment::OversizeAtom(fragment) => {
                for map in fragment.maps() {
                    if ordinary_offset(map.logical_position).is_some() {
                        consider(*map);
                    }
                }
            }
            StreamingLayoutFragment::Boundary(fragment) => {
                for map in fragment.maps() {
                    if ordinary_offset(map.logical_position).is_some() {
                        consider(*map);
                    }
                }
            }
            StreamingLayoutFragment::InlineObject(_) => {}
        }
    }
    ordered.then_some(first.zip(last)).flatten()
}

fn position_for_composite_fragments(
    fragments: &[StreamingLayoutFragment],
    position: SourcePosition,
) -> Option<Point<Pixels>> {
    let position = position.into();
    fragments.iter().find_map(|fragment| match fragment {
        StreamingLayoutFragment::Text(fragment) => fragment
            .position_for_logical_position(position)
            .ok()
            .flatten(),
        StreamingLayoutFragment::OversizeAtom(fragment) => {
            fragment.position_for_logical_position(position)
        }
        StreamingLayoutFragment::InlineObject(fragment) => {
            fragment.position_for_logical_position(position)
        }
        StreamingLayoutFragment::Boundary(fragment) => {
            fragment.position_for_logical_position(position)
        }
    })
}

fn ordinary_offset(position: StreamingLayoutPosition) -> Option<u64> {
    (position.gap == StreamingObjectGap::no_objects()).then_some(position.byte_offset)
}

fn record_ordinary_edge_fallback(
    leading_edge: Option<&StreamingLayoutMap>,
    trailing_edge: Option<&StreamingLayoutMap>,
    hit: Point<Pixels>,
    block_extent: Pixels,
    viewport_start: ByteOffset,
    leading: &mut Option<StreamingLayoutPosition>,
    trailing: &mut Option<StreamingLayoutPosition>,
    exact_viewport_start: &mut bool,
) {
    for edge in [leading_edge, trailing_edge].into_iter().flatten() {
        *exact_viewport_start |=
            ordinary_offset(edge.logical_position) == Some(viewport_start.get());
    }
    if let Some(edge) = leading_edge {
        if ordinary_offset(edge.logical_position).is_some()
            && hit.y >= edge.position.y
            && hit.y < edge.position.y + block_extent
            && hit.x < edge.position.x
        {
            leading.get_or_insert(edge.logical_position);
        }
    }
    if let Some(edge) = trailing_edge {
        if ordinary_offset(edge.logical_position).is_some()
            && hit.y >= edge.position.y
            && hit.y < edge.position.y + block_extent
            && hit.x > edge.position.x
        {
            *trailing = Some(edge.logical_position);
        }
    }
}
