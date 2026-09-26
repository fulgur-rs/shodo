//! Paragraphs: immutable results of `ParagraphBuilder::build`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::analysis::bidi::{BidiParagraph, analyze_bidi};
use crate::analysis::units::{InlineBoxInfo, Unit, UnitList, build_units};
use crate::analysis::{Item, ItemKind, process, transform};
use crate::builder::ParagraphBuilder;
use crate::font::FontCollection;
use crate::geometry::{BaselineKind, Direction, Saturation, WritingMode};
use crate::limits::{LimitExceeded, Limits, Warning, WarningKind};
use crate::mapping::OffsetMapping;
use crate::node::{NodeId, Sides};
use crate::output::Line;
use crate::sanitize;
use crate::shape::{GlyphStore, ShapedRun, shape_items};
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

pub(crate) struct ParagraphData {
    #[cfg(test)]
    pub(crate) baseline_queries: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pub(crate) cluster_queries: std::sync::atomic::AtomicUsize,
    pub(crate) id: u64,
    pub(crate) style: ParagraphStyle,
    pub(crate) limits: Limits,
    pub(crate) text: String,
    pub(crate) items: Vec<Item>,
    pub(crate) styles: Vec<InlineStyle>,
    pub(crate) glyphs: GlyphStore,
    /// Unit index for each shaping cluster and cluster index for each glyph.
    pub(crate) clusters: Vec<u32>,
    pub(crate) glyph_clusters: Vec<u32>,
    /// Unit index and node, in float ordinal order.
    pub(crate) floats: Vec<(u32, NodeId)>,
    pub(crate) runs: Vec<ShapedRun>,
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

    pub(crate) fn from_builder(
        b: ParagraphBuilder,
        cx: &mut crate::LayoutContext,
        fonts: &FontCollection,
    ) -> Result<Paragraph, LimitExceeded> {
        let ParagraphBuilder {
            mut style,
            limits,
            text,
            mut items,
            mut styles,
            mut warnings,
            offset_mapping,
            ..
        } = b;
        for s in &mut styles {
            s.font_size = sanitize_font_size(s.font_size, &mut warnings);
            sanitize::style(s, &mut warnings);
        }
        // The root inline box's interned style (index 0) is the one layout
        // reads; keep the stored paragraph style consistent with it.
        if let Some(root) = styles.first() {
            style.root = root.clone();
        }
        sanitize::items(&mut items, &mut warnings);
        let processed = process(&text, &items, &styles, offset_mapping, &limits)?;
        let processed = transform(processed, &styles, &limits, &mut warnings)?;
        let breaks = crate::analysis::breaks::analyze_breaks(&processed, &styles, &mut warnings);
        let mut sat = Saturation::default();
        let bidi = analyze_bidi(&processed.text, &style, &styles);
        let shape_items_input =
            crate::analysis::itemize::itemize(&processed, &styles, &bidi, fonts);
        let (glyphs, runs) = shape_items(
            cx,
            &shape_items_input,
            &styles,
            fonts,
            &limits,
            &mut warnings,
            &mut sat,
        )?;
        let base_level = u8::from(style.direction == Direction::Rtl);
        let UnitList {
            units,
            boxes,
            float_count,
        } = build_units(
            &processed.text,
            &processed.items,
            &runs,
            &glyphs,
            &bidi.levels,
            base_level,
            &breaks,
        );
        let mut baselines = HashMap::new();
        for item in &processed.items {
            if let ItemKind::Atomic { parent_style, .. } = item.kind
                && let Some(node) = item.node
            {
                baselines.entry(node).or_insert_with(|| {
                    baseline_kind(style.writing_mode, &styles[parent_style as usize])
                });
            }
        }
        let mut clusters = Vec::new();
        let mut glyph_clusters = vec![0; glyphs.len()];
        let mut floats = Vec::new();
        for (i, u) in units.iter().enumerate() {
            match &u.kind {
                crate::analysis::units::UnitKind::Cluster { glyphs, .. } => {
                    let cluster = clusters.len() as u32;
                    clusters.push(i as u32);
                    glyph_clusters[glyphs.start as usize..glyphs.end as usize].fill(cluster);
                }
                crate::analysis::units::UnitKind::Float { node, .. } => {
                    floats.push((i as u32, *node))
                }
                _ => {}
            }
        }
        warnings.record_saturation(&sat);
        Ok(Paragraph {
            data: Arc::new(ParagraphData {
                #[cfg(test)]
                baseline_queries: Default::default(),
                #[cfg(test)]
                cluster_queries: Default::default(),
                id: NEXT_PARAGRAPH_ID.fetch_add(1, Ordering::Relaxed),
                style,
                limits,
                text: processed.text,
                items: processed.items,
                styles,
                glyphs,
                clusters,
                glyph_clusters,
                floats,
                runs,
                units,
                boxes,
                float_count,
                base_level,
                bidi_paragraphs: bidi.paragraphs,
                mapping: processed.mapping,
                fonts: fonts.clone(),
                generations: fonts.generations(),
                warnings: warnings.take(),
                baselines,
            }),
        })
    }
}

/// The dominant baseline of a parent inline box (CSS Writing Modes 4 §4.2):
/// central in vertical typographic modes unless the text is set sideways.
fn baseline_kind(writing_mode: WritingMode, parent: &InlineStyle) -> BaselineKind {
    match writing_mode {
        WritingMode::VerticalRl | WritingMode::VerticalLr
            if parent.text_orientation != TextOrientation::Sideways =>
        {
            BaselineKind::Central
        }
        _ => BaselineKind::Alphabetic,
    }
}

fn sanitize_font_size(size: f32, warnings: &mut crate::limits::WarningSink) -> f32 {
    if !size.is_finite() {
        warnings.push(
            WarningKind::NonFiniteInput,
            "non-finite font-size replaced with 0",
        );
        0.0
    } else if size < 0.0 {
        warnings.push(
            WarningKind::NegativeInput,
            "negative font-size replaced with 0",
        );
        0.0
    } else if size > MAX_FONT_SIZE {
        warnings.push(WarningKind::Saturated, "font-size clamped to 1e6 px");
        MAX_FONT_SIZE
    } else {
        size
    }
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
            floats_placed_through: None,
            break_plan: None,
        }
    }
}

/// Outcome of [`Paragraph::next_line`].
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
