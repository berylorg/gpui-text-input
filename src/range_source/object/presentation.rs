use std::sync::Arc;

use gpui::{Hsla, Pixels, SharedString};

use super::ObjectContractError;

/// Immutable bounded visual, semantic, and activation facts for one inline object.
#[derive(Debug, PartialEq)]
pub struct InlineObjectPresentation {
    presentation_key: u64,
    display: Arc<str>,
    width: Pixels,
    height: Pixels,
    baseline: Pixels,
    background: Option<Hsla>,
    semantic_state: u64,
    activation_eligible: bool,
}

impl Clone for InlineObjectPresentation {
    fn clone(&self) -> Self {
        Self {
            presentation_key: self.presentation_key,
            display: Arc::from(self.display.as_ref()),
            width: self.width,
            height: self.height,
            baseline: self.baseline,
            background: self.background,
            semantic_state: self.semantic_state,
            activation_eligible: self.activation_eligible,
        }
    }
}

impl InlineObjectPresentation {
    /// Creates one app-neutral presentation, rejecting non-finite or invalid metrics.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        presentation_key: u64,
        display: impl Into<String>,
        width: Pixels,
        height: Pixels,
        baseline: Pixels,
        background: Option<Hsla>,
        semantic_state: u64,
        activation_eligible: bool,
    ) -> Result<Self, ObjectContractError> {
        let valid = |value: Pixels| f32::from(value).is_finite();
        if !valid(width)
            || !valid(height)
            || !valid(baseline)
            || width <= Pixels::ZERO
            || height <= Pixels::ZERO
            || baseline < Pixels::ZERO
            || baseline > height
        {
            return Err(ObjectContractError::InvalidPresentationMetrics);
        }
        Ok(Self {
            presentation_key,
            display: Arc::from(display.into()),
            width,
            height,
            baseline,
            background,
            semantic_state,
            activation_eligible,
        })
    }

    /// Returns the host-owned identity of geometry/paint-affecting presentation input.
    pub const fn presentation_key(&self) -> u64 {
        self.presentation_key
    }

    /// Returns bounded display content. It is never authoritative source text.
    pub fn display(&self) -> &str {
        &self.display
    }

    pub(crate) fn shared_display(&self) -> SharedString {
        SharedString::new(self.display.clone())
    }

    pub(crate) fn shared_clone(&self) -> Self {
        Self {
            presentation_key: self.presentation_key,
            display: self.display.clone(),
            width: self.width,
            height: self.height,
            baseline: self.baseline,
            background: self.background,
            semantic_state: self.semantic_state,
            activation_eligible: self.activation_eligible,
        }
    }

    pub(crate) fn display_allocation(&self) -> (*const u8, usize) {
        (self.display.as_ptr(), self.display.len())
    }

    /// Returns the exact inline extent.
    pub const fn width(&self) -> Pixels {
        self.width
    }

    /// Returns the exact block extent.
    pub const fn height(&self) -> Pixels {
        self.height
    }

    /// Returns the baseline offset from the top edge.
    pub const fn baseline(&self) -> Pixels {
        self.baseline
    }

    /// Returns the optional app-neutral background.
    pub const fn background(&self) -> Option<Hsla> {
        self.background
    }

    /// Returns opaque app-neutral semantic state.
    pub const fn semantic_state(&self) -> u64 {
        self.semantic_state
    }

    /// Reports whether ordinary pointer or keyboard activation is allowed.
    pub const fn activation_eligible(&self) -> bool {
        self.activation_eligible
    }

    pub(super) fn payload_bytes(&self) -> Result<usize, ObjectContractError> {
        Ok(self.display.len())
    }

    pub(crate) fn payload_allocation_bytes(&self) -> usize {
        self.display.len()
    }
}
