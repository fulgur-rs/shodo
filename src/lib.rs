//! shodo — inline formatting context engine.
//!
//! shodo lays out the inline content of one block container (a paragraph):
//! white-space processing, bidi, line breaking, alignment, inline boxes and
//! atomic inlines. Coordinates are logical (inline / block axes) and are
//! converted to physical coordinates with [`geometry::PhysicalConverter`].
#![forbid(unsafe_code)]
// Internal items are wired up incrementally; removed once all are in use.
#![allow(dead_code, unused_imports)]

pub mod font;
pub mod geometry;
pub mod limits;
pub mod mapping;
pub mod node;
pub mod style;

mod analysis;
mod builder;
mod context;
mod paragraph;
mod shape;

pub use builder::{ParagraphBuilder, RichText};
pub use context::LayoutContext;
pub use paragraph::{BreakToken, FloatCursor, Paragraph};
