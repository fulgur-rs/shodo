//! shodo — inline formatting context engine.
//!
//! shodo lays out the inline content of one block container (a paragraph):
//! white-space processing, bidi, line breaking, alignment, inline boxes and
//! atomic inlines. Coordinates are logical (inline / block axes) and are
//! converted to physical coordinates with [`geometry::PhysicalConverter`].
//!
//! The caller supplies computed styles, sizes and baselines for atomic inlines,
//! places floats, and renders the resulting fragments. CSS resolution, page
//! layout and glyph rasterization belong to the application.
//!
//! # Getting started
//!
//! Add `shodo` to your project's dependencies with `cargo add shodo`.
//! shodo requires Rust 1.89 or later. On Linux, the default system-font backend
//! needs Fontconfig development files and pkg-config.
//!
//! The basic flow is:
//!
//! 1. Keep a [`font::FontCollection`] for font registration and matching.
//! 2. Reuse one [`LayoutContext`] per thread for layout scratch space.
//! 3. Use [`RichText`] for styled spans, or [`ParagraphBuilder`] for nested
//!    inline boxes and caller-defined source identities.
//! 4. Build an immutable [`Paragraph`], then lay it out into [`Line`] values.
//!    Reuse the paragraph when only the available width changes; rebuild when
//!    its text, styles or font dependencies change.
//! 5. Read line text or render [`Line::fragments`].
//!
//! This complete example lays out styled text at a fixed width:
//!
//! ```
//! use shodo::font::FontCollection;
//! use shodo::limits::Limits;
//! use shodo::style::{InlineStyle, LineOptions, ParagraphStyle};
//! use shodo::{AtomicSizes, LayoutContext, RichText};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let limits = Limits::default();
//!     let fonts = FontCollection::new(&limits);
//!     let mut cx = LayoutContext::new();
//!     let style = InlineStyle {
//!         font_size: 16.0,
//!         ..InlineStyle::default()
//!     };
//!     let paragraph_style = ParagraphStyle {
//!         root: style.clone(),
//!         ..ParagraphStyle::default()
//!     };
//!     let paragraph = RichText::with_limits(&paragraph_style, &limits)
//!         .push("Hello world from shodo", &style)
//!         .build(&mut cx, &fonts)?;
//!
//!     let lines = paragraph.break_all(
//!         &mut cx,
//!         &LineOptions::default(),
//!         160.0,
//!         &AtomicSizes::EMPTY,
//!     );
//!     for line in &lines {
//!         println!("{}", &line.text()[line.text_range()]);
//!     }
//!     Ok(())
//! }
//! ```
//!
//! Font discovery depends on the platform and installed fonts. For repeatable
//! output, disable discovery with [`font::FontOptions`] and register bundled
//! font bytes with [`font::FontCollection::register`]. Missing fonts produce
//! warning-backed missing-glyph output, which is not a drawable substitute for
//! a real font. Read build warnings with [`Paragraph::warnings`] and layout
//! warnings with [`LayoutContext::take_warnings`].
//!
//! [`Paragraph::break_all`] is for fixed-width layout without float placement.
//! Use [`Paragraph::next_line`] for changing widths, height constraints or
//! caller-managed floats. Slice each line's own text as above: first-line
//! transforms can make [`Line::text`] differ from [`Paragraph::text`].
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
    ParagraphAnalysis,
};
pub use shape::orientation::GlyphOrientation;
