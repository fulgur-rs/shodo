//! Grapheme orientation, separate from bidi order and physical block flow.
use crate::geometry::WritingMode;
use crate::style::TextOrientation;
use icu_properties::{CodePointMapData, props::VerticalOrientation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphOrientation {
    Horizontal,
    Upright,
    /// Horizontal glyphs composed inside one vertical 1em square.
    Combined,
    SidewaysClockwise,
    SidewaysCounterClockwise,
}

pub(crate) use GlyphOrientation as RunOrientation;

/// `base` is the first scalar of the complete grapheme, including when its
/// marks cross transparent source or shaping-style boundaries.
pub(crate) fn resolve(
    mode: WritingMode,
    orientation: TextOrientation,
    base: char,
) -> RunOrientation {
    match mode {
        WritingMode::HorizontalTb => RunOrientation::Horizontal,
        WritingMode::SidewaysRl => RunOrientation::SidewaysClockwise,
        WritingMode::SidewaysLr => RunOrientation::SidewaysCounterClockwise,
        WritingMode::VerticalRl | WritingMode::VerticalLr => match orientation {
            TextOrientation::Upright => RunOrientation::Upright,
            TextOrientation::Sideways => RunOrientation::SidewaysClockwise,
            TextOrientation::Mixed => {
                if CodePointMapData::<VerticalOrientation>::new().get(base)
                    == VerticalOrientation::Rotated
                {
                    RunOrientation::SidewaysClockwise
                } else {
                    // U, Tu and Tr all use vertical typesetting. Alternate
                    // glyphs for the transformed classes are a shaping task.
                    RunOrientation::Upright
                }
            }
        },
    }
}
