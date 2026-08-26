use gpui::{Context, Window};

use crate::{RangeTextInput, RangeTextInputError};

impl RangeTextInput {
    pub fn attach_active_inline_object_surface(
        &mut self,
        expected: crate::RealizedInlineObjectAnchor,
    ) -> Result<crate::InlineObjectSurfaceAttachment, RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if self.attached_inline_object_surface.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        let active = self.active_object.ok_or(RangeTextInputError::Stale)?;
        if active.anchor != expected {
            return Err(RangeTextInputError::Stale);
        }
        let id = self.next_inline_object_surface_attachment;
        self.next_inline_object_surface_attachment =
            id.checked_add(1).ok_or(RangeTextInputError::Busy)?;
        self.attached_inline_object_surface = Some((id, expected));
        Ok(crate::InlineObjectSurfaceAttachment {
            id,
            anchor: expected,
        })
    }

    pub fn dismiss_active_inline_object_surface(
        &mut self,
        attachment: crate::InlineObjectSurfaceAttachment,
        dismissal: crate::InlineObjectSurfaceDismissal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if self.attached_inline_object_surface != Some((attachment.id, attachment.anchor))
            || self.active_object.map(|active| active.anchor) != Some(attachment.anchor)
        {
            return Err(RangeTextInputError::Stale);
        }
        match dismissal {
            crate::InlineObjectSurfaceDismissal::RefocusObject => {
                self.focus(window);
                self.attached_inline_object_surface = None;
                cx.notify();
            }
            crate::InlineObjectSurfaceDismissal::ClearObject => {
                let candidate = self.prepare_active_object_transition(
                    super::transition::ActiveObjectTransition::Clear(
                        crate::InlineObjectRealizationLossReason::FocusLost,
                    ),
                )?;
                self.attached_inline_object_surface = None;
                self.commit_active_object_transition(candidate, cx);
            }
        }
        Ok(())
    }

    pub(super) fn install_active_object(&mut self, active: Option<super::ActiveInlineObject>) {
        self.active_object = active;
        if self
            .attached_inline_object_surface
            .is_some_and(|(_, anchor)| active.map(|active| active.anchor) != Some(anchor))
        {
            self.attached_inline_object_surface = None;
        }
    }

    pub fn remove_active_inline_object(
        &mut self,
        expected: crate::RealizedInlineObjectAnchor,
        cx: &mut Context<Self>,
    ) -> Result<crate::MutationKey, RangeTextInputError> {
        let active = self.active_object.ok_or(RangeTextInputError::Stale)?;
        if active.anchor != expected {
            return Err(RangeTextInputError::Stale);
        }
        let range = crate::SourceRange::new(active.leading, active.trailing)
            .map_err(|_| RangeTextInputError::Stale)?;
        self.begin_source_replacement(range, String::new(), crate::MutationKind::Edit, cx)
    }
}
