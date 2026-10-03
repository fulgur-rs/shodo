//! shodo — inline formatting context engine.
//!
//! shodo lays out the inline content of one block container (a paragraph):
//! white-space processing, bidi, line breaking, alignment, inline boxes and
//! atomic inlines. Coordinates are logical (inline / block axes) and are
//! converted to physical coordinates with [`geometry::PhysicalConverter`].
//!
//! # Integration guides
//!
//! Start with the [integration guide] for fonts, incremental layout, output
//! ownership, limits and hit testing. These links are pinned to a reviewed
//! repository revision, so their content stays stable when `main` changes.
//!
//! | Task | Guide |
//! | --- | --- |
//! | Integrate fonts and incremental line layout | [Integration guide][integration guide] |
//! | Limit lines by processed grapheme count | [Character-limited lines] |
//! | Supply exact normal and first-line styles | [First-line styles] |
//! | Track glyphs shared by several source nodes | [Shared glyph ownership] |
//! | Interpret horizontal metrics, spacing and source positions | [Horizontal output] |
//! | Configure Japanese breaks, spacing and hanging | [Japanese typography] |
//! | Paint vertical, sideways and combined text | [Vertical output] |
//! | Build and paint ruby readings | [Ruby layout] |
//! | Paint text and decorations | [Text paint] and [PNG rendering example] |
//! | Render emoji and color glyphs | [Emoji output] |
//! | Expose retained text to assistive technology | [Accessibility output] |
//! | Place, retry and withdraw floats | [Float integration] |
//!
//! The [documentation index] also links to development checks. Design and
//! measurement records describe their recorded scope; they are not current
//! API contracts. Use the API reference on this page for method signatures.
//!
//! [integration guide]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/integration.md
//! [Character-limited lines]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/grapheme-limited-lines.md
//! [First-line styles]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/first-line-style-contract.md
//! [Shared glyph ownership]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/shared-glyph-contract.md
//! [Horizontal output]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/horizontal-layout-contracts.md
//! [Japanese typography]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/japanese-layout.md
//! [Vertical output]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/vertical-layout.md
//! [Ruby layout]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/ruby.md
//! [Text paint]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/paint-styles.md
//! [PNG rendering example]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/png-render-sample.md
//! [Emoji output]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/emoji.md
//! [Accessibility output]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/accessibility.md
//! [Float integration]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/float-integration-harness.md
//! [documentation index]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/README.md
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
