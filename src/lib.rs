//! shodo — inline formatting context engine.
//!
//! shodo lays out the inline content of one block container (a paragraph):
//! white-space processing, bidi, line breaking, alignment, inline boxes and
//! atomic inlines. Coordinates are logical (inline / block axes) and are
//! converted to physical coordinates with [`geometry::PhysicalConverter`].
#![forbid(unsafe_code)]

pub mod font;
pub mod geometry;
pub mod limits;
pub mod mapping;
pub mod node;
pub mod style;

mod analysis;
mod builder;
mod context;
mod line;
mod output;
mod paragraph;
mod sanitize;
mod shape;

pub use builder::{ParagraphBuilder, RichText};
pub use context::LayoutContext;
pub use output::{
    AnchorFragment, AtomicFragment, BreakReason, Cluster, Fragment, Glyph, GlyphRunView, Glyphs,
    InlineBoxFragment, Line,
};
pub use paragraph::{
    AtomicIntrinsic, AtomicIntrinsics, AtomicSize, AtomicSizes, BreakPlan, BreakToken, FloatClear,
    FloatCursor, FloatIntrinsic, FloatSide, IntrinsicSizes, LineConstraint, LineResult, Paragraph,
};
