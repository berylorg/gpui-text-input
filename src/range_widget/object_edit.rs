use gpui::Context;

use crate::{
    MutationKey, MutationPageItem, MutationPositions, MutationProposal, RangeTextInput,
    RangeTextInputError,
};

impl RangeTextInput {
    pub fn insert_inline_object_at_selection(
        &mut self,
        id: crate::InlineObjectId,
        order: crate::InlineObjectOrder,
        retained_bytes: usize,
        presentation_bytes: usize,
        cx: &mut Context<Self>,
    ) -> Result<crate::MutationKey, RangeTextInputError> {
        if !self.enabled || self.read_only {
            return Err(RangeTextInputError::ReadOnly);
        }
        if self.pending_local_mutation.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        if retained_bytes > self.config.mutation_limits.max_page_object_bytes()
            || presentation_bytes > self.config.mutation_limits.max_page_presentation_bytes()
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        let (selection, caret, replacement, removed, proofs) = {
            let surface = self
                .interactive_surface()
                .ok_or(RangeTextInputError::Busy)?;
            let selection = surface.selection();
            let replacement = selection.range().map_err(|_| RangeTextInputError::Stale)?;
            let selected = surface.object_selected_by(selection);
            if !replacement.is_empty() && selected.is_none() {
                return Err(RangeTextInputError::Pending);
            }
            let mut proofs = Vec::with_capacity(2);
            for position in [replacement.start(), replacement.end()] {
                if proofs
                    .iter()
                    .any(|proof: &crate::range_edit::SourcePositionProof| {
                        proof.position() == position
                    })
                {
                    continue;
                }
                proofs.push(crate::range_edit::SourcePositionProof::from_surface_pages(
                    self.config.binding,
                    position,
                    surface.pages(),
                    surface.object_pages(),
                )?);
            }
            let removed = selected
                .map(|object| crate::ObjectTarget::new(replacement, object.id(), object.order()))
                .transpose()?;
            (selection, surface.caret(), replacement, removed, proofs)
        };
        let preceding = match replacement.start().gap {
            crate::InlineObjectGap::Between { preceding, .. }
            | crate::InlineObjectGap::After(preceding) => Some(preceding),
            crate::InlineObjectGap::NoObjects | crate::InlineObjectGap::Before(_) => None,
        };
        let following = match replacement.end().gap {
            crate::InlineObjectGap::Between { following, .. }
            | crate::InlineObjectGap::Before(following) => Some(following),
            crate::InlineObjectGap::NoObjects | crate::InlineObjectGap::After(_) => None,
        };
        if preceding.is_some_and(|neighbor| neighbor.order() >= order)
            || following.is_some_and(|neighbor| neighbor.order() <= order)
            || preceding.is_some_and(|neighbor| neighbor.id() == id)
            || following.is_some_and(|neighbor| neighbor.id() == id)
        {
            return Err(RangeTextInputError::Stale);
        }
        let object = crate::SuccessorObject::new(
            id,
            replacement.start().byte_offset,
            order,
            retained_bytes,
            presentation_bytes,
        );
        let change = removed.map_or(crate::ObjectChange::Insert { object }, |target| {
            crate::ObjectChange::Replace { target, object }
        });
        let neighbor = crate::InlineObjectNeighbor::new(id, order);
        let intended_gap = following
            .map_or_else(
                || Ok(crate::InlineObjectGap::after(neighbor)),
                |following| crate::InlineObjectGap::between(neighbor, following),
            )
            .map_err(|_| RangeTextInputError::Stale)?;
        let intended = crate::SourcePosition::new(replacement.start().byte_offset, intended_gap);
        let key = MutationKey::new(
            self.config.binding.binding(),
            self.config.binding.revision(),
            self.next_local_operation()?,
        );
        let predecessor = MutationPositions::new(caret, selection.anchor, selection.head);
        let proposal =
            MutationProposal::new(key, crate::MutationKind::Edit, predecessor, replacement, 0);
        self.begin_local_mutation(
            proposal,
            vec![MutationPageItem::Object(change)],
            MutationPositions::collapsed(intended),
            cx,
        )?;
        self.admitted_edit_proofs = proofs;
        Ok(key)
    }
}
