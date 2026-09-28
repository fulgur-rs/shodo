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
use crate::limits::{LimitExceeded, LimitKind, Limits, Warning, WarningKind, WarningSink};
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
            mut first_line_styles,
            mut warnings,
            offset_mapping,
            rubies,
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
        let id = NEXT_PARAGRAPH_ID.fetch_add(1, Ordering::Relaxed);
        let has_first_line = style.first_line.is_some() || !first_line_styles.is_empty();
        if has_first_line {
            Limits::check(
                limits.max_styles,
                LimitKind::Styles,
                styles.len() as u64 * 2,
            )?;
        }
        let alternate_styles = has_first_line.then(|| {
            let first = style.first_line.as_ref().unwrap_or(&style.root);
            styles
                .iter()
                .enumerate()
                .map(
                    |(index, original)| match first_line_styles.remove(&(index as u32)) {
                        Some(resolved) => resolved_first_line_style(original, &resolved),
                        None => first_line_style(original, &style.root, first),
                    },
                )
                .collect::<Vec<_>>()
        });
        let processed = process(&text, &items, &styles, offset_mapping, &limits)?;
        let source_cuts = alternate_styles
            .as_ref()
            .map(|_| crate::analysis::breaks::source_cursor_ranges(&processed));
        let mut processed = transform(
            processed,
            &styles,
            &limits,
            &mut warnings,
            style.writing_mode,
        )?;
        if alternate_styles.is_none() {
            processed.source_spans = Vec::new();
        }
        let mut sat = Saturation::default();
        let mut data = build_data(
            style.clone(),
            &limits,
            limits.max_shaped_glyphs,
            processed,
            styles,
            id,
            cx,
            fonts,
            &mut warnings,
            &mut sat,
        )?;
        data.ruby_inputs = rubies.clone();
        if let Some(mut alternate_styles) = alternate_styles {
            for s in &mut alternate_styles {
                s.font_size = sanitize_font_size(s.font_size, &mut warnings);
                sanitize::style(s, &mut warnings);
            }
            let mut remaining = limits.clone();
            remaining.max_text_bytes = limits
                .max_text_bytes
                .map(|max| max.saturating_sub(data.text.len() as u64));
            remaining.max_items = limits
                .max_items
                .map(|max| max.saturating_sub(data.items.len() as u64));
            // The transient common input is bounded independently; a shrinking
            // transform may fit the remaining retained-text budget even when
            // its input is larger. Every output append uses the remaining cap.
            let mut input_limits = remaining.clone();
            input_limits.max_text_bytes = limits.max_text_bytes;
            let alternate = process(&text, &items, &data.styles, offset_mapping, &input_limits)
                .and_then(|p| {
                    transform(
                        p,
                        &alternate_styles,
                        &remaining,
                        &mut warnings,
                        style.writing_mode,
                    )
                })
                .map_err(|mut e| {
                    if e.kind == LimitKind::TextBytes
                        && let Some(limit) = limits.max_text_bytes
                    {
                        e.actual += data.text.len() as u64;
                        e.limit = limit;
                    }
                    if e.kind == LimitKind::Items
                        && let Some(limit) = limits.max_items
                    {
                        e.actual += data.items.len() as u64;
                        e.limit = limit;
                    }
                    e
                })?;
            let mut alternate_style = style;
            alternate_style.root = alternate_styles[0].clone();
            alternate_style.first_line = None;
            let remaining_glyphs = limits
                .max_shaped_glyphs
                .map(|max| max.saturating_sub(data.glyphs.len() as u64));
            let mut alternate = build_data(
                alternate_style,
                &limits,
                remaining_glyphs,
                alternate,
                alternate_styles,
                id,
                cx,
                fonts,
                &mut warnings,
                &mut sat,
            )
            .map_err(|mut e| {
                if e.kind == LimitKind::ShapedGlyphs
                    && let Some(limit) = limits.max_shaped_glyphs
                {
                    e.actual += data.glyphs.len() as u64;
                    e.limit = limit;
                }
                e
            })?;
            alternate.ruby_inputs = rubies;
            finalize_data(&mut data, cx, &mut warnings, &mut sat);
            finalize_data(&mut alternate, cx, &mut warnings, &mut sat);
            let mut normal_search = 0;
            let mut normal_cursors: Vec<_> = alternate
                .units
                .iter()
                .map(|u| {
                    normal_cursor(
                        &data,
                        &alternate,
                        u,
                        source_cuts.as_ref().unwrap(),
                        &mut normal_search,
                    )
                })
                .collect();
            normal_cursors.push(Some(data.units.len() as u32));
            for i in 0..alternate.units.len() {
                if normal_cursors[i + 1].is_some() {
                    continue;
                }
                let class = alternate.units[i].break_after;
                let min_content = alternate.units[i].emergency_min_content;
                alternate.units[i].break_after = crate::analysis::units::BreakClass::Prohibited;
                alternate.units[i].emergency_min_content = false;
                if matches!(
                    class,
                    crate::analysis::units::BreakClass::Allowed
                        | crate::analysis::units::BreakClass::Emergency
                ) {
                    // Preserve the transformed opportunity beyond markers
                    // inside a source grapheme whose trailing scalar was consumed.
                    for j in i + 1..alternate.units.len() {
                        use crate::analysis::units::UnitKind;
                        if !matches!(
                            alternate.units[j].kind,
                            UnitKind::Float { .. }
                                | UnitKind::Absolute { .. }
                                | UnitKind::Open { .. }
                                | UnitKind::Close { .. }
                                | UnitKind::BidiControl
                        ) {
                            break;
                        }
                        if normal_cursors[j + 1].is_some() {
                            if alternate.units[j].break_after
                                == crate::analysis::units::BreakClass::Prohibited
                            {
                                alternate.units[j].break_after = class;
                                alternate.units[j].emergency_min_content = min_content;
                            }
                            break;
                        }
                    }
                }
            }
            data.source_spans = Vec::new();
            alternate.source_spans = Vec::new();
            data.first_line = Some(FirstLineData {
                data: Arc::new(alternate),
                alternate_cursors: normal_cursors
                    .iter()
                    .enumerate()
                    .filter_map(|(i, u)| u.map(|u| (u, i as u32)))
                    .collect(),
                normal_cursors,
            });
        } else {
            data.source_spans = Vec::new();
            finalize_data(&mut data, cx, &mut warnings, &mut sat);
        }
        warnings.record_saturation(&sat);
        data.warnings = warnings.take();
        Ok(Paragraph {
            data: Arc::new(data),
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn build_data(
    style: ParagraphStyle,
    limits: &Limits,
    glyph_budget: Option<u64>,
    processed: crate::analysis::whitespace::Processed,
    styles: Vec<InlineStyle>,
    id: u64,
    cx: &mut crate::LayoutContext,
    fonts: &FontCollection,
    warnings: &mut WarningSink,
    sat: &mut Saturation,
) -> Result<ParagraphData, LimitExceeded> {
    let mut shape_limits = limits.clone();
    shape_limits.max_shaped_glyphs = glyph_budget;
    let mut combine_spans = crate::analysis::combine::prepare(
        &processed.text,
        &processed.items,
        &styles,
        style.writing_mode,
    );
    let mut breaks = crate::analysis::breaks::analyze_breaks(&processed, &styles, warnings);
    for opportunity in &mut breaks.opportunities {
        let index = combine_spans.partition_point(|span| span.text.end <= opportunity.offset);
        if let Some(span) = combine_spans.get(index)
            && span.text.start < opportunity.offset
        {
            opportunity.class = crate::analysis::units::BreakClass::Prohibited;
            opportunity.min_content = false;
        }
    }
    let used_direction = crate::analysis::bidi::used_root_direction(&style, &styles[0]);
    let bidi_text = crate::analysis::bidi::upright_analysis_text(
        &processed,
        &styles,
        style.writing_mode,
        &combine_spans,
    );
    let bidi = analyze_bidi(
        bidi_text.as_deref().unwrap_or(&processed.text),
        &style,
        &styles,
        used_direction,
    );
    let mut shape_items_input = crate::analysis::itemize::itemize(
        &processed,
        &styles,
        &bidi,
        &breaks,
        fonts,
        style.writing_mode,
        &combine_spans,
    );
    crate::shape::select_combined_widths(
        cx,
        &mut shape_items_input,
        &styles,
        fonts,
        style.writing_mode,
        &shape_limits,
    );
    let (glyphs, runs) = shape_items(
        cx,
        &shape_items_input,
        &styles,
        fonts,
        style.writing_mode,
        &shape_limits,
        warnings,
        sat,
    )?;
    let base_level = u8::from(used_direction == Direction::Rtl);
    let style_metrics: Vec<_> = styles
        .iter()
        .map(|s| crate::line::font_metrics::resolve(fonts, s, warnings))
        .collect();
    let combine_geometry = crate::analysis::combine::geometry(
        &processed.text,
        &processed.items,
        &styles,
        &combine_spans,
        &glyphs,
        &runs,
        fonts,
        &style_metrics,
        sat,
    );
    let UnitList {
        mut units,
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
    for (i, unit) in units.iter_mut().enumerate() {
        let index = combine_spans.partition_point(|span| span.text.end <= unit.text.start);
        if let Some(span) = combine_spans.get_mut(index)
            && span.text.start <= unit.text.start
        {
            if matches!(
                unit.kind,
                crate::analysis::units::UnitKind::Cluster { .. }
                    | crate::analysis::units::UnitKind::Tab
            ) {
                unit.combine = Some(index as u32);
                if span.units.is_empty() {
                    span.units.start = i;
                }
                span.units.end = i + 1;
                unit.level = bidi.levels[span.text.start as usize];
            }
            if unit.text.end < span.text.end {
                unit.break_after = crate::analysis::units::BreakClass::Prohibited;
                unit.emergency_min_content = false;
            }
        }
    }
    for span in &combine_spans {
        for unit in &mut units[span.units.clone()] {
            if unit.combine.is_some() {
                unit.slice_advance = crate::geometry::LayoutUnit::ZERO;
            }
        }
        if !span.units.is_empty() {
            units[span.units.end - 1].slice_advance =
                crate::geometry::LayoutUnit::from_f32_round(span.em, sat);
        }
    }
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
    let data = ParagraphData {
        ruby_inputs: Vec::new(),
        #[cfg(test)]
        spacing_setup_visits: Default::default(),
        #[cfg(test)]
        baseline_queries: Default::default(),
        #[cfg(test)]
        cluster_queries: Default::default(),
        #[cfg(test)]
        cursor_queries: Default::default(),
        #[cfg(test)]
        window_queries: Default::default(),
        first_line: None,
        source_spans: processed.source_spans,
        id,
        style,
        limits: limits.clone(),
        text: processed.text,
        items: processed.items,
        styles,
        combine_spans,
        combine_geometry,
        style_metrics,
        unit_spacing: Vec::new(),
        punctuation: Vec::new(),
        last_content_unit: None,
        internal_autospace_gaps: Vec::new(),
        needs_spacing: false,
        spacing_tree: Default::default(),
        glyphs,
        clusters: Vec::new(),
        glyph_clusters: Vec::new(),
        selectable_clusters: Vec::new(),
        shaping_barriers: Vec::new(),
        floats: Vec::new(),
        runs,
        shape_items: shape_items_input,
        breaks,
        units,
        boxes,
        float_count,
        base_level,
        bidi_paragraphs: bidi.paragraphs,
        mapping: processed.mapping,
        fonts: fonts.clone(),
        generations: fonts.generations(),
        warnings: Vec::new(),
        baselines,
    };
    Ok(data)
}

fn finalize_data(
    data: &mut ParagraphData,
    cx: &mut crate::LayoutContext,
    warnings: &mut WarningSink,
    sat: &mut Saturation,
) {
    crate::line::reshape::initialize_slices(data, cx, warnings, sat);
    data.spacing_tree = crate::line::autospace::Tree::build(data);
    data.punctuation = crate::line::punctuation::build(data, sat);
    (data.unit_spacing, data.internal_autospace_gaps) = crate::line::spacing::build(data, sat);
    data.last_content_unit = crate::line::spacing::last_content(data);
    data.needs_spacing = crate::line::spacing::needed(data);
    let mut clusters = Vec::new();
    let mut glyph_clusters = vec![0; data.glyphs.len()];
    let mut floats = Vec::new();
    let mut selectable_clusters = Vec::new();
    let mut shaping_barriers = Vec::new();
    let mut previous_cluster = None;
    for (i, u) in data.units.iter().enumerate() {
        match &u.kind {
            crate::analysis::units::UnitKind::Cluster { glyphs, .. } => {
                selectable_clusters.push(i as u32);
                let same = previous_cluster
                    .is_some_and(|previous: usize| u.shares_cluster(&data.units[previous]));
                previous_cluster = Some(i);
                if same {
                    continue;
                }
                let cluster = clusters.len() as u32;
                clusters.push(i as u32);
                glyph_clusters[glyphs.start as usize..glyphs.end as usize].fill(cluster);
            }
            crate::analysis::units::UnitKind::Float { node, .. } => floats.push((i as u32, *node)),
            crate::analysis::units::UnitKind::Atomic { .. }
            | crate::analysis::units::UnitKind::ForcedBreak
            | crate::analysis::units::UnitKind::BlockInInline { .. }
            | crate::analysis::units::UnitKind::Tab
            | crate::analysis::units::UnitKind::BidiControl => shaping_barriers.push(i as u32),
            crate::analysis::units::UnitKind::Open { box_index }
            | crate::analysis::units::UnitKind::Close { box_index } => {
                let b = &data.boxes[*box_index as usize];
                let start = matches!(u.kind, crate::analysis::units::UnitKind::Open { .. });
                if crate::analysis::itemize::inline_boundary_breaks_shaping(
                    &data.styles[b.style as usize],
                    &b.edges,
                    start,
                ) {
                    shaping_barriers.push(i as u32);
                }
            }
            _ => {}
        }
    }
    data.shaping_barriers = shaping_barriers;
    data.clusters = clusters;
    data.selectable_clusters = selectable_clusters;
    data.glyph_clusters = glyph_clusters;
    data.floats = floats;
}

fn normal_cursor(
    normal: &ParagraphData,
    alternate: &ParagraphData,
    u: &Unit,
    source_cuts: &[std::ops::RangeInclusive<u32>],
    search: &mut usize,
) -> Option<u32> {
    use crate::analysis::units::UnitKind;
    use crate::mapping::TransformSpan;
    let source = TransformSpan::source_position(&alternate.source_spans, u.text.start);
    let cut = source_cuts.partition_point(|cut| *cut.end() < source);
    if !source_cuts
        .get(cut)
        .is_some_and(|cut| cut.contains(&source))
    {
        return None;
    }
    if TransformSpan::map_position(&alternate.source_spans, source) != u.text.start {
        return None;
    }
    let pos = TransformSpan::map_position(&normal.source_spans, source);
    if TransformSpan::source_position(&normal.source_spans, pos) != source {
        return None;
    }
    // Both sets preserve source/item order. Exact mapped cuts are monotone,
    // including empty markers sharing a source offset, so never restart a group.
    while let Some(n) = normal.units.get(*search) {
        #[cfg(test)]
        normal.cursor_queries.fetch_add(1, Ordering::Relaxed);
        if n.text.start > pos {
            return None;
        }
        if n.text.start == pos
            && match (&n.kind, &u.kind) {
                (UnitKind::Cluster { .. }, UnitKind::Cluster { .. }) => true,
                _ => {
                    n.item == u.item
                        && std::mem::discriminant(&n.kind) == std::mem::discriminant(&u.kind)
                }
            }
        {
            return Some(*search as u32);
        }
        *search += 1;
    }
    None
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
