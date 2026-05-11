use gpui::{Hsla, hsla};

/// App-neutral colors used by the reusable text-input renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct TextInputTheme {
    /// Text color. When absent, the current GPUI text style color is used.
    pub text: Option<Hsla>,
    /// Placeholder text color.
    pub placeholder: Hsla,
    /// Selection highlight color.
    pub selection: Hsla,
    /// Caret color.
    pub caret: Hsla,
    /// Underline color used for marked IME composition text.
    pub marked_underline: Hsla,
    /// Text color used for opaque atom ranges.
    pub atom_text: Hsla,
    /// Background color used for opaque atom ranges.
    pub atom_background: Option<Hsla>,
}

impl Default for TextInputTheme {
    fn default() -> Self {
        Self {
            text: None,
            placeholder: hsla(0.0, 0.0, 0.55, 0.72),
            selection: hsla(0.59, 0.68, 0.50, 0.34),
            caret: hsla(0.59, 0.82, 0.62, 1.0),
            marked_underline: hsla(0.59, 0.82, 0.62, 1.0),
            atom_text: hsla(0.58, 0.78, 0.72, 1.0),
            atom_background: Some(hsla(0.58, 0.55, 0.28, 0.85)),
        }
    }
}
