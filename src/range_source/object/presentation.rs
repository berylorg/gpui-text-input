use gpui::{Hsla, Pixels, SharedString};

use super::ObjectContractError;

/// Immutable bounded visual, semantic, and activation facts for one inline object.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineObjectPresentation {
    presentation_key: u64,
    display: SharedString,
    width: Pixels,
    height: Pixels,
    baseline: Pixels,
    background: Option<Hsla>,
    semantic_state: u64,
    activation_eligible: bool,
}

impl InlineObjectPresentation {
    /// Creates one app-neutral presentation, rejecting non-finite or invalid metrics.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        presentation_key: u64,
        display: impl Into<SharedString>,
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
            display: display.into(),
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
    pub fn display(&self) -> &SharedString {
        &self.display
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
}
