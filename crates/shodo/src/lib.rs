//! shodo — inline formatting context engine.
//!
//! shodo lays out the inline content of one block container (a paragraph):
//! white-space processing, bidi, line breaking, alignment, inline boxes and
//! atomic inlines. Coordinates are logical (inline / block axes) and are
//! converted to physical coordinates with [`geometry::PhysicalConverter`].
#![forbid(unsafe_code)]

pub mod accessibility;
pub mod font;
pub mod geometry;
pub mod hit;
pub mod limits;
pub mod mapping;
pub mod node;
pub mod ruby;
pub mod style;
pub use ruby::{
    Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyError, RubyHit, RubyLevel,
    RubyMerge, RubyOverhang, RubyPosition, RubySpan, RubyStyle, RubyVisibility,
};

mod analysis;
mod builder;
mod context;
mod hashing;
mod line;
mod output;
mod paragraph;
mod sanitize;
mod shape;
#[cfg(test)]
mod test_support;

pub use analysis::breaks::{LineBreakContext, LineBreakOverride, SoftBreakOpportunity};
pub use builder::{ParagraphBuilder, RichText};
pub use context::LayoutContext;
pub use output::{
    AnchorFragment, AtomicFragment, BreakReason, Cluster, ClusterFlags, DecorationRect, Fragment,
    Glyph, GlyphRunView, GlyphTransform, Glyphs, InlineBoxFragment, Line, LineMetrics, PaintSpan,
    RubyAnnotationView, RubyTransform, TextCombination,
};
pub use paragraph::{
    AtomicIntrinsic, AtomicIntrinsics, AtomicSize, AtomicSizes, BreakPlan, BreakToken, FloatClear,
    FloatCursor, FloatIntrinsic, FloatSide, IntrinsicSizes, LineConstraint, LineResult, Paragraph,
};
pub use shape::orientation::GlyphOrientation;
