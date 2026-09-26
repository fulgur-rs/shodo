//! CSS descriptors kept separately from a font's intrinsic metadata.

use super::FontError;
use crate::style::FontStyle;

/// Resolved `@font-face` descriptors. Ranges are inclusive. An empty
/// `unicode_ranges` list covers all Unicode scalar values.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFaceDescriptor {
    pub family: String,
    /// CSS font-weight range, from 1 through 1000.
    pub weight: (f32, f32),
    /// CSS font-width percentages; positive and finite.
    pub width: (f32, f32),
    pub style: FontStyle,
    pub unicode_ranges: Vec<(u32, u32)>,
}

impl Default for FontFaceDescriptor {
    fn default() -> Self {
        Self {
            family: String::new(),
            weight: (400., 400.),
            width: (100., 100.),
            style: FontStyle::Normal,
            unicode_ranges: Vec::new(),
        }
    }
}

impl FontFaceDescriptor {
    pub(super) fn validate(&self) -> Result<(), FontError> {
        let ordered = |(min, max): (f32, f32)| min.is_finite() && max.is_finite() && min <= max;
        if self.family.trim().is_empty()
            || !ordered(self.weight)
            || self.weight.0 < 1.
            || self.weight.1 > 1000.
            || !ordered(self.width)
            || self.width.0 <= 0.
            || matches!(self.style, FontStyle::Oblique(a) if !a.is_finite() || !(-90. ..=90.).contains(&a))
            || self
                .unicode_ranges
                .iter()
                .any(|&(min, max)| min > max || max > 0x10ffff)
        {
            return Err(FontError::Malformed("invalid CSS font descriptor"));
        }
        Ok(())
    }
}
