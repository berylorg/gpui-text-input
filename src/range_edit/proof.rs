use super::*;
use crate::{ObjectPageId, ObjectResidency, PageId, RangeResidency};

/// Constant-size proof that one composite position was observed in coherent bounded sources.
///
/// The constructor validates a UTF-8 boundary against one exact text page and the inline gap
/// against one exact-anchor object page. The proof retains only revision and page identities.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SourcePositionProof {
    binding: RangeBinding,
    position: SourcePosition,
    text_page: Option<PageId>,
    object_page: ObjectPageId,
}

impl SourcePositionProof {
    pub(crate) fn from_surface_pages(
        binding: RangeBinding,
        position: SourcePosition,
        text_pages: &[crate::RangePage],
        object_pages: &[crate::ObjectPage],
    ) -> Result<Self, MutationError> {
        if position.byte_offset.get() > binding.extent().byte_len() {
            return Err(MutationError::PositionOutsideExtent);
        }
        let text_page = if position.byte_offset.get() == 0
            || position.byte_offset.get() == binding.extent().byte_len()
        {
            None
        } else {
            let mut covered = false;
            let mut proven = None;
            for page in text_pages {
                if !page.range().contains_offset(position.byte_offset) {
                    continue;
                }
                covered = true;
                let local =
                    usize::try_from(position.byte_offset.get() - page.range().start().get())
                        .map_err(|_| MutationError::MissingTextBoundaryProof)?;
                if page.text().is_char_boundary(local) {
                    proven = Some(page.id());
                    break;
                }
            }
            match (covered, proven) {
                (_, Some(page)) => Some(page),
                (true, None) => return Err(MutationError::InvalidTextBoundaryProof),
                (false, None) => return Err(MutationError::MissingTextBoundaryProof),
            }
        };
        let object_page = object_pages
            .iter()
            .find(|page| crate::object_residency::page_proves_gap(page, position))
            .map(crate::ObjectPage::id)
            .ok_or(MutationError::InvalidObjectGapProof)?;
        Ok(Self {
            binding,
            position,
            text_page,
            object_page,
        })
    }

    pub(crate) fn from_admitted_sources(
        binding: RangeBinding,
        position: SourcePosition,
        text: &RangeResidency,
        objects: &ObjectResidency,
    ) -> Result<Self, MutationError> {
        if text.binding() != binding || objects.binding() != binding {
            return Err(MutationError::StalePositionProof);
        }
        let scalar =
            text.prove_scalar_boundary(position.byte_offset)
                .map_err(|error| match error {
                    crate::ScalarBoundaryProofError::NotScalarBoundary(_) => {
                        MutationError::InvalidTextBoundaryProof
                    }
                    crate::ScalarBoundaryProofError::OutsideExtent(_) => {
                        MutationError::PositionOutsideExtent
                    }
                    crate::ScalarBoundaryProofError::Unavailable(_) => {
                        MutationError::MissingTextBoundaryProof
                    }
                })?;
        let object_page = objects
            .prove_position_gap(position)
            .ok_or(MutationError::InvalidObjectGapProof)?;
        Ok(Self {
            binding,
            position,
            text_page: scalar.source_page(),
            object_page,
        })
    }

    pub const fn binding(self) -> RangeBinding {
        self.binding
    }

    pub const fn position(self) -> SourcePosition {
        self.position
    }

    pub const fn text_page(self) -> Option<PageId> {
        self.text_page
    }

    pub const fn object_page(self) -> ObjectPageId {
        self.object_page
    }
}

/// Exact successor caret and directed selection positions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationPositions {
    caret: SourcePosition,
    selection_anchor: SourcePosition,
    selection_head: SourcePosition,
}

impl MutationPositions {
    pub const fn collapsed(position: SourcePosition) -> Self {
        Self::new(position, position, position)
    }

    pub const fn new(
        caret: SourcePosition,
        selection_anchor: SourcePosition,
        selection_head: SourcePosition,
    ) -> Self {
        Self {
            caret,
            selection_anchor,
            selection_head,
        }
    }

    pub const fn caret(self) -> SourcePosition {
        self.caret
    }

    pub const fn selection_anchor(self) -> SourcePosition {
        self.selection_anchor
    }

    pub const fn selection_head(self) -> SourcePosition {
        self.selection_head
    }
}

/// Three bounded coherent-source proofs carried by a committed successor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MutationPositionProofs {
    caret: SourcePositionProof,
    selection_anchor: SourcePositionProof,
    selection_head: SourcePositionProof,
}

impl MutationPositionProofs {
    pub(crate) fn new(
        binding: RangeBinding,
        positions: MutationPositions,
        caret: SourcePositionProof,
        selection_anchor: SourcePositionProof,
        selection_head: SourcePositionProof,
    ) -> Result<Self, MutationError> {
        for (expected, proof) in [
            (positions.caret(), caret),
            (positions.selection_anchor(), selection_anchor),
            (positions.selection_head(), selection_head),
        ] {
            if proof.binding() != binding {
                return Err(MutationError::StalePositionProof);
            }
            if proof.position() != expected {
                return Err(MutationError::WrongSuccessorPositionProof);
            }
        }
        Ok(Self {
            caret,
            selection_anchor,
            selection_head,
        })
    }

    pub(crate) const fn as_array(self) -> [SourcePositionProof; 3] {
        [self.caret, self.selection_anchor, self.selection_head]
    }
}

/// One committed successor whose compact positions are proven by coherent bounded sources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationCommit {
    binding: RangeBinding,
    positions: MutationPositions,
    proofs: MutationPositionProofs,
}

impl MutationCommit {
    pub(crate) fn from_admitted_sources(
        binding: RangeBinding,
        positions: MutationPositions,
        text: &RangeResidency,
        objects: &ObjectResidency,
    ) -> Result<Self, MutationError> {
        let caret =
            SourcePositionProof::from_admitted_sources(binding, positions.caret(), text, objects)?;
        let anchor = SourcePositionProof::from_admitted_sources(
            binding,
            positions.selection_anchor(),
            text,
            objects,
        )?;
        let head = SourcePositionProof::from_admitted_sources(
            binding,
            positions.selection_head(),
            text,
            objects,
        )?;
        let proofs = MutationPositionProofs::new(binding, positions, caret, anchor, head)?;
        Self::new(binding, positions, proofs)
    }

    pub(crate) fn new(
        binding: RangeBinding,
        positions: MutationPositions,
        proofs: MutationPositionProofs,
    ) -> Result<Self, MutationError> {
        MutationPositionProofs::new(
            binding,
            positions,
            proofs.caret,
            proofs.selection_anchor,
            proofs.selection_head,
        )?;
        Ok(Self {
            binding,
            positions,
            proofs,
        })
    }

    pub const fn binding(self) -> RangeBinding {
        self.binding
    }

    pub const fn positions(self) -> MutationPositions {
        self.positions
    }

    pub(crate) const fn proofs(self) -> MutationPositionProofs {
        self.proofs
    }
}
