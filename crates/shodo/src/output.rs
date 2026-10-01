//! Line layout output.

mod glyphs;
mod line;
mod owned_bytes;
#[cfg(test)]
pub(crate) mod owner_probe;
mod paint;
pub(crate) mod ruby;
pub use paint::{DecorationRect, PaintSpan};
pub use ruby::{RubyAnnotationView, RubyTransform};

use std::ops::Range;
use std::sync::Arc;

use crate::font::FontId;
use crate::geometry::{LayoutUnit, LogicalRect};
use crate::line::fragments::GlyphSource;
use crate::line::fragments::{self, FragmentRecord};
use crate::node::{NodeId, OutOfFlowKind};
use crate::paragraph::{BreakToken, FloatCursor, ParagraphData};
use crate::shape::GlyphStore;

/// Why a line ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakReason {
    /// At a soft break opportunity.
    Regular,
    /// At a forced break (`<br>`, preserved newline).
    Forced,
    /// Inside a word because of `overflow-wrap`.
    Emergency,
    /// Before a block-level box inside inline content.
    BlockInInline,
    /// At the end of the paragraph.
    End,
}

/// One laid-out line. Owns a reference to its paragraph's data, so it can
/// outlive the `Paragraph` handle and be sent between threads.
#[derive(Clone)]
pub struct Line {
    #[cfg(test)]
    pub(crate) _clone_probe: clone_probe::CloneProbe,
    pub(crate) ruby: Vec<ruby::RubyAnnotationRecord>,
    pub(crate) ruby_caret_gaps: Vec<crate::ruby::align::CaretGap>,
    pub(crate) data: Arc<ParagraphData>,
    pub(crate) break_token: BreakToken,
    pub(crate) reason: BreakReason,
    pub(crate) units: Range<u32>,
    text_range: Range<u32>,
    pub(crate) inline_size: LayoutUnit,
    pub(crate) trailing_whitespace: LayoutUnit,
    pub(crate) hanging_end: LayoutUnit,
    pub(crate) hanging_start: LayoutUnit,
    visible_hyphen: Option<u32>,
    pub(crate) block_size: LayoutUnit,
    pub(crate) baseline: LayoutUnit,
    pub(crate) ascent: LayoutUnit,
    pub(crate) descent: LayoutUnit,
    pub(crate) block_offset: f32,
    pub(crate) displaced: Vec<(NodeId, FloatCursor)>,
    pub(crate) fragments: Vec<FragmentRecord>,
    pub(crate) block_shifts: Vec<LayoutUnit>,
    pub(crate) empty: bool,
    pub(crate) tabs: Vec<fragments::TabSlot>,
    combinations: Vec<TextCombination>,
    pub(crate) positions: Option<(u32, Vec<LayoutUnit>)>,
    pub(crate) glyph_spacing: Option<(u32, Vec<crate::line::spacing::GlyphSpacing>)>,
    pub(crate) overlay: Option<Box<GlyphStore>>,
    pub(crate) overlay_clusters: Box<[OverlayCluster]>,
    pub(crate) overlay_runs: Box<[crate::shape::ShapedRun]>,
    pub(crate) pending_overlays: Vec<crate::line::reshape::EdgeOverlay>,
}

/// Final line-box extents and root font edges, in line-local logical block coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetrics {
    /// Distances from the dominant baseline to logical block-start/end.
    pub ascent: f32,
    pub descent: f32,
    /// Dominant baseline position from logical block-start.
    pub baseline: f32,
    /// Root font edge on the line-over side (block-end in `vertical-lr`).
    pub text_over: f32,
    /// Root font edge on the line-under side (block-start in `vertical-lr`).
    pub text_under: f32,
}

/// One external typographic character composed from horizontal text.
/// Glyphs retain their original source owners and internal shaping clusters.
#[derive(Clone, Debug, PartialEq)]
pub struct TextCombination {
    /// Processed-text range in this accepted line's text dataset.
    pub text_range: Range<usize>,
    /// The 1em square, in logical coordinates relative to the line's top.
    /// Add the line's block offset before converting to physical coordinates.
    pub square: LogicalRect,
}

/// A positioned piece of a line.
#[derive(Clone, Copy, Debug)]
pub enum Fragment<'a> {
    RubyAnnotation(RubyAnnotationView<'a>),
    /// A glyph run with at least one glyph; nonpainting controls do not yield empty runs.
    GlyphRun(GlyphRunView<'a>),
    Atomic(AtomicFragment),
    InlineBox(InlineBoxFragment),
    OutOfFlowAnchor(AnchorFragment),
}

/// The part of an inline box on one line. With `box-decoration-break:
/// slice`, only the first fragment has the start edge and only the last has
/// the end edge. Border and content widths exclude collapsible spaces hanging
/// at the line end; preserved spaces remain inside the box even when hanging.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineBoxFragment {
    pub node: NodeId,
    /// Border box. Block sides cover the content area plus block padding and
    /// border, which do not affect the line height (CSS 2.1 §10.8.1).
    pub rect: LogicalRect,
    pub content_rect: LogicalRect,
    /// Cumulative logical inline offset from the start of the slice-composite
    /// box, excluding margins. Populated by
    /// [`crate::Paragraph::break_all`] and
    /// [`crate::Paragraph::break_all_with_grapheme_limit`]
    /// for `box-decoration-break: slice`; `None` for clone and streaming lines.
    pub slice_offset: Option<f32>,
    /// Dominant baseline in logical block coordinates from block-start of the line box.
    pub baseline: f32,
    pub has_start_edge: bool,
    pub has_end_edge: bool,
    /// Whether the box's own `direction` opposes the paragraph's, putting
    /// its logical inline-start on the paragraph's inline-end side.
    pub start_edge_is_reversed: bool,
    /// Index of the parent inline box fragment in [`Line::fragments`].
    pub parent: Option<usize>,
    /// Primary font of the box, for text-decoration metrics.
    pub font: FontId,
    pub font_size: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtomicFragment {
    pub node: NodeId,
    pub margin_rect: LogicalRect,
    pub border_rect: LogicalRect,
    /// Baseline position from the top of the line box.
    pub baseline: f32,
}

/// Static position of an out-of-flow box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorFragment {
    pub node: NodeId,
    pub kind: OutOfFlowKind,
    pub inline_position: f32,
}

/// A run of glyphs from one font, one element and one bidi level.
#[derive(Clone, Copy, Debug)]
pub struct GlyphRunView<'a> {
    source: GlyphSource,
    block_shift: LayoutUnit,
    line: &'a Line,
    record: &'a FragmentRecord,
    run: u32,
    glyphs: (u32, u32),
    item: u32,
    text: (u32, u32),
}

/// Local outline coordinates (x right, y down) to logical inline/block
/// displacement. The outline has already been scaled to the run's font size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphTransform {
    pub inline_x: f32,
    pub inline_y: f32,
    pub block_x: f32,
    pub block_y: f32,
}

/// One positioned glyph. `inline_position` is the glyph origin from the
/// container's content edge; `block_offset` is relative to the baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub inline_position: f32,
    pub block_offset: f32,
    /// Layout advance, including the cluster's assigned spacing. Half spacing
    /// can precede its ink; `inline_position` remains the actual glyph origin.
    pub advance: f32,
    pub cluster: u32,
}

/// A shaping cluster with both its laid-out and original advance.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    pub text_range: Range<usize>,
    pub advance: f32,
    pub shaping_advance: f32,
    /// First scalar in the processed source range (SHY retains U+00AD).
    pub source_char: Option<char>,
    pub flags: ClusterFlags,
}

/// Source classification; whitespace and punctuation describe the first
/// scalar. A multi-character shaping cluster is emphasis-excluded only when
/// every constituent character is excluded. Renderers still place emphasis
/// once per typographic character, rather than once per shaping cluster.
/// Combined text's internal clusters are excluded here; use
/// [`Line::text_combinations`] for its single external emphasis target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClusterFlags {
    pub whitespace: bool,
    pub punctuation: bool,
    pub synthetic_hyphen: bool,
    pub emphasis_excluded: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct OverlayCluster {
    pub(crate) glyphs: Range<u32>,
    pub(crate) text: Range<u32>,
}

/// Iterator over the glyphs of a run, with random access through `get`.
#[derive(Clone, Debug)]
pub struct Glyphs<'a> {
    view: GlyphRunView<'a>,
    next: u32,
    end: u32,
}

impl<'a> GlyphRunView<'a> {
    fn natural_advance(&self, index: usize) -> f32 {
        let store = match self.source {
            GlyphSource::Shared => &self.data().glyphs,
            _ => self.line.overlay.as_deref().expect("overlay store"),
        };
        store.advance[self.glyphs.0 as usize + index].to_f32()
    }
    fn data(&self) -> &'a ParagraphData {
        &self.line.data
    }
}

// Test-only, zero-sized observation of the real derived Line::clone path.
#[cfg(test)]
pub(crate) mod clone_probe {
    use std::cell::Cell;
    thread_local! { static COUNT: Cell<usize> = const { Cell::new(0) }; }
    pub(crate) struct CloneProbe;
    impl Clone for CloneProbe {
        fn clone(&self) -> Self {
            COUNT.with(|c| c.set(c.get() + 1));
            Self
        }
    }
    pub(crate) fn reset() {
        COUNT.with(|c| c.set(0));
    }
    pub(crate) fn count() -> usize {
        COUNT.with(Cell::get)
    }
}

// Counts actual materialization, independently of the retry implementation.
#[cfg(test)]
pub(crate) mod construction_probe {
    use std::cell::Cell;
    thread_local! { static COUNT: Cell<usize> = const { Cell::new(0) }; }
    pub(crate) fn record() {
        COUNT.with(|c| c.set(c.get() + 1));
    }
    pub(crate) fn reset() {
        COUNT.with(|c| c.set(0));
    }
    pub(crate) fn count() -> usize {
        COUNT.with(Cell::get)
    }
}
