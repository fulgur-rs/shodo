//! Paragraphs: immutable results of `ParagraphBuilder::build`.

mod build;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::analysis::bidi::{BidiParagraph, analyze_bidi};
use crate::analysis::units::{InlineBoxInfo, Unit, UnitList, build_units};
use crate::analysis::{Item, ItemKind, transform_with_base_scopes};
use crate::builder::ParagraphBuilder;
use crate::font::FontCollection;
use crate::geometry::{BaselineKind, Direction, Saturation, WritingMode};
use crate::limits::{LimitExceeded, LimitKind, Limits, Warning, WarningKind, WarningSink};
use crate::mapping::OffsetMapping;
use crate::node::{NodeId, Sides};
use crate::output::Line;
use crate::sanitize;
use crate::shape::{GlyphStore, ShapedRun, shape_items_with_base_scopes};
use crate::style::{InlineStyle, ParagraphStyle, TextOrientation};

static NEXT_PARAGRAPH_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_ATOMIC_REVISION: AtomicU64 = AtomicU64::new(1);

/// Largest accepted font size in px; larger values are clamped.
const MAX_FONT_SIZE: f32 = 1.0e6;

/// Position in a paragraph where a line starts. Opaque and cheap to copy;
/// valid only for the paragraph that produced it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BreakToken {
    pub(crate) para: u64,
    pub(crate) unit: u32,
    pub(crate) flags: u8,
}

impl BreakToken {
    pub(crate) const FIRST_LINE: u8 = 1;
    pub(crate) const AFTER_FORCED: u8 = 2;
}

/// Marks how many floats of a paragraph have been handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FloatCursor(pub(crate) u32);

impl FloatCursor {
    /// The cursor just before this float (for withdrawing it), or `None`
    /// for the first float.
    pub fn before(self) -> Option<FloatCursor> {
        self.0.checked_sub(1).map(FloatCursor)
    }
}

pub(crate) struct FirstLineData {
    pub(crate) data: Arc<ParagraphData>,
    /// Exact alternate-unit cursors in the normal set; absent cuts are prohibited.
    pub(crate) normal_cursors: Vec<Option<u32>>,
    pub(crate) alternate_cursors: Vec<(u32, u32)>,
}

impl FirstLineData {
    pub(crate) fn alternate_cursor(&self, normal: u32) -> Option<usize> {
        let index = self.alternate_cursors.partition_point(|(u, _)| *u < normal);
        self.alternate_cursors
            .get(index)
            .filter(|(u, _)| *u == normal)
            .map(|(_, u)| *u as usize)
    }
}

pub(crate) struct ParagraphData {
    pub(crate) ruby: crate::ruby::prepare::RubyData,
    pub(crate) ruby_inputs: Vec<crate::ruby::builder::RubyInput>,
    #[cfg(test)]
    pub(crate) spacing_setup_visits: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) baseline_queries: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) cluster_queries: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) cursor_queries: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) window_queries: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) edge_shape_calls: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) edge_shape_bytes: std::sync::atomic::AtomicUsize,
    pub(crate) first_line: Option<FirstLineData>,
    pub(crate) source_spans: Vec<crate::mapping::TransformSpan>,
    pub(crate) id: u64,
    pub(crate) style: ParagraphStyle,
    pub(crate) limits: Limits,
    pub(crate) text: String,
    pub(crate) items: Vec<Item>,
    pub(crate) styles: Vec<InlineStyle>,
    pub(crate) combine_spans: Vec<crate::analysis::combine::CombineSpan>,
    pub(crate) combine_geometry: crate::analysis::combine::Geometry,
    pub(crate) style_metrics: Vec<crate::line::font_metrics::StyleMetrics>,
    pub(crate) unit_spacing: Vec<crate::line::spacing::UnitSpacing>,
    pub(crate) punctuation: Vec<crate::line::punctuation::Punctuation>,
    pub(crate) last_content_unit: Option<usize>,
    pub(crate) internal_autospace_gaps: Vec<crate::line::autospace::Gap>,
    pub(crate) needs_spacing: bool,
    pub(crate) spacing_tree: crate::line::autospace::Tree,
    pub(crate) glyphs: GlyphStore,
    /// Unit index for each shaping cluster and cluster index for each glyph.
    pub(crate) clusters: Vec<u32>,
    pub(crate) selectable_clusters: Vec<u32>,
    pub(crate) shaping_barriers: Vec<u32>,
    pub(crate) glyph_clusters: Vec<u32>,
    /// Unit index and node, in float ordinal order.
    pub(crate) floats: Vec<(u32, NodeId)>,
    pub(crate) runs: Vec<ShapedRun>,
    pub(crate) shape_items: Vec<crate::analysis::itemize::ShapeItem>,
    pub(crate) breaks: crate::analysis::breaks::BreakAnalysis,
    pub(crate) units: Vec<Unit>,
    pub(crate) boxes: Vec<InlineBoxInfo>,
    pub(crate) float_count: u32,
    /// Coordinate direction of the block; independent of plaintext paragraphs.
    pub(crate) base_level: u8,
    pub(crate) bidi_paragraphs: Vec<BidiParagraph>,
    pub(crate) mapping: Option<OffsetMapping>,
    pub(crate) fonts: FontCollection,
    pub(crate) generations: (u64, Option<u64>),
    pub(crate) warnings: Vec<Warning>,
    pub(crate) baselines: HashMap<NodeId, BaselineKind>,
}

impl ParagraphData {
    /// Center a composition on its containing inline's text-over/under edges,
    /// before the inline's vertical-align displacement is applied.
    pub(crate) fn combine_center_shift(&self, style: u32) -> f32 {
        let metrics = self.style_metrics[style as usize];
        let (over, under) = if self.styles[style as usize].text_orientation
            == crate::style::TextOrientation::Sideways
        {
            (metrics.metrics.ascent, metrics.metrics.descent)
        } else {
            metrics
                .vertical_metrics
                .map_or((metrics.size / 2.0, metrics.size / 2.0), |v| {
                    (v.ascent, v.descent)
                })
        };
        -(over - under) / 2.0
    }

    pub(crate) fn combine_at_text(
        &self,
        offset: u32,
    ) -> Option<&crate::analysis::combine::CombineSpan> {
        let index = self
            .combine_spans
            .partition_point(|span| span.text.end <= offset);
        self.combine_spans
            .get(index)
            .filter(|span| span.text.start <= offset)
    }

    pub(crate) fn bidi_paragraph_at_unit(&self, unit: usize) -> Option<&BidiParagraph> {
        let pos = self.units.get(unit)?.text.start;
        self.bidi_paragraph_at_text(pos)
    }

    pub(crate) fn bidi_paragraph_at_text(&self, pos: u32) -> Option<&BidiParagraph> {
        let index = self.bidi_paragraphs.partition_point(|p| p.text.end <= pos);
        self.bidi_paragraphs.get(index)
    }

    pub(crate) fn baseline_kind(&self, node: NodeId) -> Option<BaselineKind> {
        #[cfg(test)]
        self.baseline_queries.fetch_add(1, Ordering::Relaxed);
        self.baselines.get(&node).copied()
    }
}

/// The analyzed and shaped inline content of one block container.
/// Immutable; clones share data and keep the same id.
#[derive(Clone)]
pub struct Paragraph {
    pub(crate) data: Arc<ParagraphData>,
}

impl fmt::Debug for Paragraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Paragraph")
            .field("id", &self.data.id)
            .field("text", &self.data.text)
            .finish()
    }
}

impl Paragraph {
    pub fn id(&self) -> u64 {
        self.data.id
    }

    pub fn start_token(&self) -> BreakToken {
        BreakToken {
            para: self.data.id,
            unit: 0,
            flags: BreakToken::FIRST_LINE,
        }
    }

    /// (shared layer, document layer) font generations at build time. If
    /// the collection's current generations differ, rebuild the paragraph.
    pub fn font_generations(&self) -> (u64, Option<u64>) {
        self.data.generations
    }

    pub fn warnings(&self) -> &[Warning] {
        &self.data.warnings
    }

    pub fn offset_mapping(&self) -> Option<&OffsetMapping> {
        self.data.mapping.as_ref()
    }

    /// The processed text (after white-space collapsing).
    pub fn text(&self) -> &str {
        &self.data.text
    }

    /// Which baseline of atomic inline `node` the caller must supply in
    /// `AtomicSizes`: the dominant baseline of its parent inline box.
    pub fn required_baseline(&self, node: NodeId) -> Option<BaselineKind> {
        self.data.baseline_kind(node)
    }
}

macro_rules! first_line_properties {
    ($copy:ident) => {
        $copy!(
            font_families,
            font_size,
            font_weight,
            font_width,
            font_style,
            font_variations,
            font_features,
            font_kerning,
            font_variant_ligatures,
            font_variant_caps,
            font_variant_numeric,
            font_variant_east_asian,
            font_variant_position,
            font_variant_alternates,
            font_optical_sizing,
            font_synthesis,
            font_size_adjust,
            lang,
            line_height,
            letter_spacing,
            word_spacing,
            text_transform,
            text_emphasis
        );
    };
}

pub(crate) fn first_line_style(
    original: &InlineStyle,
    root: &InlineStyle,
    first: &InlineStyle,
) -> InlineStyle {
    let mut result = original.clone();
    macro_rules! inherit {
        ($($field:ident),* $(,)?) => { $(if original.$field == root.$field { result.$field = first.$field.clone(); })* };
    }
    first_line_properties!(inherit);
    macro_rules! inherit_paint {
        ($($field:ident),*) => { $(if original.paint.$field == root.paint.$field {
            result.paint.$field = first.paint.$field;
        })* };
    }
    inherit_paint!(color, underline, strikethrough);
    result
}

fn resolved_first_line_style(original: &InlineStyle, resolved: &InlineStyle) -> InlineStyle {
    let mut result = original.clone();
    macro_rules! copy {
        ($($field:ident),* $(,)?) => { $(result.$field = resolved.$field.clone();)* };
    }
    first_line_properties!(copy);
    result.paint = resolved.paint;
    result
}

/// Size of an atomic inline (image, inline-block), supplied by the caller
/// before line layout. `baseline` is measured from the top of the margin box
/// and must be of the kind reported by [`Paragraph::required_baseline`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AtomicSize {
    pub inline_size: f32,
    pub block_size: f32,
    pub baseline: Option<f32>,
    pub margins: Sides,
}

/// Sizes of atomic inlines by node. The generation changes on every insert.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AtomicSizes {
    map: BTreeMap<NodeId, AtomicSize>,
    generation: u64,
    pub(crate) revision: u64,
}

impl AtomicSizes {
    pub const EMPTY: AtomicSizes = AtomicSizes {
        map: BTreeMap::new(),
        generation: 0,
        revision: 0,
    };

    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, node: NodeId, size: AtomicSize) {
        self.map.insert(node, size);
        self.generation = self.generation.wrapping_add(1);
        self.revision = NEXT_ATOMIC_REVISION.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get(&self, node: NodeId) -> Option<&AtomicSize> {
        self.map.get(&node)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Minimum and maximum intrinsic inline sizes in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IntrinsicSizes {
    pub min_content: f32,
    pub max_content: f32,
}

/// Intrinsic margin-box widths supplied by the caller; do not add margins again.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AtomicIntrinsic {
    pub min_content: f32,
    pub max_content: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloatSide {
    Left,
    Right,
    #[default]
    InlineStart,
    InlineEnd,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloatClear {
    #[default]
    None,
    Left,
    Right,
    Both,
    InlineStart,
    InlineEnd,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FloatIntrinsic {
    pub min_content: f32,
    pub max_content: f32,
    pub side: FloatSide,
    pub clear: FloatClear,
}

/// Intrinsic margin-box widths of atomic inlines and floats by node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AtomicIntrinsics {
    pub(crate) atomics: BTreeMap<NodeId, AtomicIntrinsic>,
    pub(crate) floats: BTreeMap<NodeId, FloatIntrinsic>,
}
impl AtomicIntrinsics {
    pub const EMPTY: Self = Self {
        atomics: BTreeMap::new(),
        floats: BTreeMap::new(),
    };
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert_atomic(&mut self, node: NodeId, value: AtomicIntrinsic) {
        self.atomics.insert(node, value);
    }
    pub fn insert_float(&mut self, node: NodeId, value: FloatIntrinsic) {
        self.floats.insert(node, value);
    }
}

/// A precomputed set of break positions for `text-wrap: balance | pretty`.
/// Produced by [`Paragraph::plan_breaks`]; mismatching inputs safely fall
/// back to greedy layout.
#[derive(Clone, Debug, PartialEq)]
pub struct BreakPlan {
    pub(crate) para: u64,
    pub(crate) width: f32,
    pub(crate) atomics_generation: u64,
    pub(crate) atomics_revision: u64,
    pub(crate) options: crate::style::LineOptions,
    pub(crate) ends: Vec<u32>,
}

/// Space available to one line.
#[derive(Clone, Copy, Debug)]
pub struct LineConstraint<'a> {
    pub available_inline_size: f32,
    /// Inline-start inset from floats, relative to the content box.
    pub inline_start_offset: f32,
    /// Block position of the line within the container; copied to the line.
    pub block_offset: f32,
    pub max_block_size: Option<f32>,
    /// Maximum processed Unicode grapheme clusters on this line. A value of
    /// zero still accepts one indivisible unit so a continuation can progress.
    /// Atomic inlines and preserved tabs count as one each. `None` has no
    /// character limit. A shaping or transform group may exceed the limit.
    pub max_graphemes: Option<usize>,
    pub floats_placed_through: Option<FloatCursor>,
    pub break_plan: Option<&'a BreakPlan>,
}

impl LineConstraint<'_> {
    pub fn new(available_inline_size: f32) -> Self {
        Self {
            available_inline_size,
            inline_start_offset: 0.0,
            block_offset: 0.0,
            max_block_size: None,
            max_graphemes: None,
            floats_placed_through: None,
            break_plan: None,
        }
    }
}

/// Outcome of [`Paragraph::next_line`].
// Keep the public by-value line result without a heap allocation on every line.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
#[non_exhaustive]
pub enum LineResult {
    Line(Line),
    /// No content is left.
    Done,
    /// The line would be taller than `max_block_size`.
    BlockSizeExceeded {
        needed_block_size: f32,
    },
    /// A float was reached; place it and call again from `line_start`.
    FloatEncountered {
        node: NodeId,
        line_start: BreakToken,
        inline_position: f32,
        float_cursor: FloatCursor,
    },
    /// A block-level box inside inline content was reached; lay it out and
    /// continue from `token_after`.
    BlockInInline {
        node: NodeId,
        token_after: BreakToken,
    },
    /// The token belongs to another paragraph or is out of range.
    InvalidToken,
}
