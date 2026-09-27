//! Line layout output.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use peniko::FontData;

use crate::font::FontId;
use crate::geometry::{BaselineKind, LayoutUnit, LogicalRect, Saturation};
use crate::line::Scan;
use crate::line::fragments::GlyphSource;
use crate::line::fragments::{self, FragmentRecord, RecordKind};
use crate::node::{NodeId, OutOfFlowKind};
use crate::paragraph::{AtomicSizes, BreakToken, FloatCursor, Paragraph, ParagraphData};
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
    pub(crate) data: Arc<ParagraphData>,
    pub(crate) break_token: BreakToken,
    pub(crate) reason: BreakReason,
    pub(crate) units: Range<u32>,
    text_range: Range<u32>,
    pub(crate) inline_size: LayoutUnit,
    pub(crate) hanging_end: LayoutUnit,
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
    pub(crate) positions: Option<(u32, Vec<LayoutUnit>)>,
    pub(crate) glyph_spacing: Option<(u32, Vec<crate::line::spacing::GlyphSpacing>)>,
    pub(crate) overlay: Option<Box<GlyphStore>>,
    pub(crate) overlay_clusters: Box<[OverlayCluster]>,
    pub(crate) overlay_runs: Box<[crate::shape::ShapedRun]>,
    pub(crate) pending_overlays: Vec<crate::line::reshape::EdgeOverlay>,
}

impl fmt::Debug for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Line")
            .field("units", &self.units)
            .field("reason", &self.reason)
            .field("inline_size", &self.inline_size())
            .finish()
    }
}

/// Final line-box extents and root font edges, in line-local block coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetrics {
    /// Distances from the alphabetic baseline to the line-box top/bottom.
    pub ascent: f32,
    pub descent: f32,
    pub baseline: f32,
    pub text_over: f32,
    pub text_under: f32,
}

impl Line {
    pub fn metrics(&self) -> LineMetrics {
        let baseline = self.baseline.to_f32();
        let root = self.data.style_metrics[0].metrics;
        LineMetrics {
            ascent: baseline,
            descent: self.block_size.to_f32() - baseline,
            baseline,
            text_over: baseline - root.ascent,
            text_under: baseline + root.descent,
        }
    }
    /// Leading hanging amount; punctuation hanging is reserved for Japanese
    /// typography. Preserved trailing whitespace is exposed by `hang_end`.
    pub fn hang_start(&self) -> f32 {
        0.0
    }
    pub fn hang_end(&self) -> f32 {
        self.hanging_end.to_f32()
    }
    /// Bounds of nominal glyph ink and painted box geometry, relative to this
    /// line's top. Renderer-added stroke, antialiasing and decorations can
    /// extend these bounds; block_offset is applied by the caller.
    pub fn overflow_rect(&self) -> LogicalRect {
        use skrifa::{
            MetadataProvider,
            instance::{LocationRef, Size},
            raw::types::GlyphId,
        };
        let mut bounds: Option<LogicalRect> = None;
        let mut include = |rect: LogicalRect| {
            if rect.inline_size <= 0.0 || rect.block_size <= 0.0 {
                return;
            }
            bounds = Some(bounds.map_or(rect, |previous| {
                let left = previous.inline_start.min(rect.inline_start);
                let top = previous.block_start.min(rect.block_start);
                LogicalRect {
                    inline_start: left,
                    block_start: top,
                    inline_size: (previous.inline_start + previous.inline_size)
                        .max(rect.inline_start + rect.inline_size)
                        - left,
                    block_size: (previous.block_start + previous.block_size)
                        .max(rect.block_start + rect.block_size)
                        - top,
                }
            }));
        };
        for fragment in self.fragments() {
            match fragment {
                Fragment::GlyphRun(run) => {
                    let data = run.font_data();
                    let font = data
                        .as_ref()
                        .and_then(|d| skrifa::FontRef::from_index(d.data.as_ref(), d.index).ok());
                    let metrics = font.as_ref().map(|f| {
                        f.glyph_metrics(
                            Size::new(run.font_size()),
                            LocationRef::new(run.normalized_coords()),
                        )
                    });
                    for (index, glyph) in run.glyphs().enumerate() {
                        if let Some(b) = metrics
                            .as_ref()
                            .and_then(|m| m.bounds(GlyphId::new(glyph.id)))
                        {
                            let skew = run.skew().unwrap_or(0.0).to_radians().tan();
                            let (mut left, mut right) = (
                                b.x_min + (skew * b.y_min).min(skew * b.y_max),
                                b.x_max + (skew * b.y_min).max(skew * b.y_max),
                            );
                            if self.data.base_level % 2 == 1 {
                                let natural = run.natural_advance(index);
                                (left, right) = (natural - right, natural - left);
                            }
                            include(LogicalRect {
                                inline_start: glyph.inline_position + left,
                                inline_size: right - left,
                                block_start: run.baseline() + glyph.block_offset - b.y_max,
                                block_size: b.y_max - b.y_min,
                            });
                        } else if metrics.is_none() {
                            let m = run.metrics();
                            include(LogicalRect {
                                inline_start: glyph.inline_position,
                                inline_size: run.natural_advance(index),
                                block_start: run.baseline() + glyph.block_offset - m.ascent,
                                block_size: m.ascent + m.descent,
                            });
                        }
                    }
                }
                Fragment::Atomic(a) => include(a.border_rect),
                Fragment::InlineBox(b) => include(b.rect),
                Fragment::OutOfFlowAnchor(_) => {}
            }
        }
        bounds.unwrap_or_default()
    }
    pub(crate) fn new(
        para: &Paragraph,
        token: BreakToken,
        scan: Scan,
        origin: LayoutUnit,
        block_offset: f32,
        atomics: &AtomicSizes,
        sat: &mut Saturation,
    ) -> Line {
        let data = &para.data;
        let m = data.style_metrics[0].metrics;
        let flags = match scan.reason {
            BreakReason::Forced => BreakToken::AFTER_FORCED,
            _ => 0,
        };
        let origin_units = token.unit as usize..scan.end;
        let visible_hyphen = scan
            .overlays
            .iter()
            .find_map(|w| w.hyphen.as_ref().map(|text| text.start));
        let (mut records, tabs) = fragments::build(
            data,
            origin_units,
            scan.hang_start,
            &scan.widths,
            origin,
            atomics,
            visible_hyphen,
        );
        crate::line::autospace::exclude_from_boxes(data, &mut records, &scan.autospace_gaps, sat);
        if let Some(leading) = &scan.leading {
            for record in &mut records {
                let RecordKind::Atomic { size, unit, .. } = &record.kind else {
                    continue;
                };
                let natural = LayoutUnit::from_f32_round(
                    size.inline_size.max(0.0) + size.margins.inline_sum(),
                    sat,
                );
                let before = leading[*unit as usize - token.unit as usize];
                let before = if record.level % 2 != data.base_level % 2 {
                    record.inline_size.sub(natural, sat).sub(before, sat)
                } else {
                    before
                };
                record.inline_start = record.inline_start.add(before, sat);
                record.inline_size = natural;
            }
        }
        // Resource-limited whole clusters can overlap following transparent
        // markers in source order. Their complete source extent still belongs
        // to this line. Cache it so public range queries remain constant-time.
        let text_range = data.units[token.unit as usize..scan.end]
            .iter()
            .fold(None, |range, u| {
                Some(range.map_or_else(
                    || u.text.clone(),
                    |range: Range<u32>| range.start.min(u.text.start)..range.end.max(u.text.end),
                ))
            })
            .unwrap_or(0..0);
        let hanging_end = data.units[scan.hang_start..scan.end]
            .iter()
            .zip(&scan.widths[scan.hang_start - token.unit as usize..])
            .filter(|(unit, _)| {
                matches!(
                    unit.kind,
                    crate::analysis::units::UnitKind::Cluster { space: true, .. }
                )
            })
            .fold(LayoutUnit::ZERO, |sum, (_, w)| sum.add(*w, sat));
        Line {
            data: Arc::clone(&para.data),
            break_token: BreakToken {
                para: data.id,
                unit: scan.end as u32,
                flags,
            },
            reason: scan.reason,
            units: token.unit..scan.end as u32,
            text_range,
            inline_size: scan.content,
            hanging_end,
            visible_hyphen,
            block_size: LayoutUnit::ZERO,
            baseline: LayoutUnit::ZERO,
            ascent: LayoutUnit::from_f32_round(m.ascent, sat),
            descent: LayoutUnit::from_f32_round(m.descent, sat),
            block_offset,
            displaced: Vec::new(),
            block_shifts: vec![LayoutUnit::ZERO; records.len()],
            fragments: records,
            empty: false,
            positions: None,
            glyph_spacing: None,
            overlay: None,
            overlay_clusters: Box::default(),
            overlay_runs: Box::default(),
            pending_overlays: scan.overlays,
            tabs,
        }
    }

    pub(crate) fn measure_metrics(&mut self, sat: &mut Saturation) {
        let metrics = crate::line::metrics::measure(
            &self.data,
            self.units.start as usize..self.units.end as usize,
            &self.fragments,
            &self.overlay_runs,
            sat,
        );
        self.block_size = metrics.block_size;
        self.baseline = metrics.baseline;
        self.block_shifts = metrics.shifts;
        self.empty = metrics.empty;
    }

    pub fn break_token(&self) -> BreakToken {
        self.break_token
    }

    pub fn break_reason(&self) -> BreakReason {
        self.reason
    }

    /// Whether this is the last line of the paragraph or of a forced-break
    /// section (the line `text-align-last` applies to).
    pub fn is_last(&self) -> bool {
        matches!(
            self.reason,
            BreakReason::Forced | BreakReason::End | BreakReason::BlockInInline
        )
    }

    /// Width of the content, excluding hanging trailing spaces, text-indent
    /// and the inline-start offset.
    pub fn inline_size(&self) -> f32 {
        self.inline_size.to_f32()
    }

    /// Line advance (distance to the next line).
    pub fn block_size(&self) -> f32 {
        self.block_size.to_f32()
    }

    pub fn block_offset(&self) -> f32 {
        self.block_offset
    }

    /// Position of a baseline, from the top of the line box. Only the
    /// alphabetic baseline comes from font data; the others are derived from
    /// the strut's ascent and descent.
    pub fn baseline(&self, kind: BaselineKind) -> f32 {
        let alphabetic = self.baseline.to_f32();
        let (ascent, descent) = (self.ascent.to_f32(), self.descent.to_f32());
        match kind {
            BaselineKind::Alphabetic => alphabetic,
            BaselineKind::Central => alphabetic - (ascent - descent) / 2.0,
            BaselineKind::Ideographic => alphabetic + descent,
            BaselineKind::Hanging => alphabetic - 0.8 * ascent,
        }
    }

    /// The complete processed text set used by this line. `::first-line`
    /// transforms can make this differ from [`Paragraph::text`].
    pub fn text(&self) -> &str {
        &self.data.text
    }

    /// Mapping for this line's processed text set, when enabled at build.
    pub fn offset_mapping(&self) -> Option<&crate::mapping::OffsetMapping> {
        self.data.mapping.as_ref()
    }

    /// Range of [`Self::text`] covered by this line.
    pub fn text_range(&self) -> Range<usize> {
        self.text_range.start as usize..self.text_range.end as usize
    }

    /// Floats reported for this line whose anchors ended up after its end.
    pub fn displaced_floats(&self) -> &[(NodeId, FloatCursor)] {
        &self.displaced
    }
}

/// A positioned piece of a line.
#[derive(Clone, Copy, Debug)]
pub enum Fragment<'a> {
    GlyphRun(GlyphRunView<'a>),
    Atomic(AtomicFragment),
    InlineBox(InlineBoxFragment),
    OutOfFlowAnchor(AnchorFragment),
}

/// The part of an inline box on one line. With `box-decoration-break:
/// slice`, only the first fragment has the start edge and only the last has
/// the end edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineBoxFragment {
    pub node: NodeId,
    /// Border box. Block sides cover the content area plus block padding and
    /// border, which do not affect the line height (CSS 2.1 §10.8.1).
    pub rect: LogicalRect,
    pub content_rect: LogicalRect,
    pub has_start_edge: bool,
    pub has_end_edge: bool,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClusterFlags {
    pub whitespace: bool,
    pub punctuation: bool,
    pub synthetic_hyphen: bool,
    pub emphasis_excluded: bool,
}

impl ClusterFlags {
    fn from_source(source: &str, synthetic_hyphen: bool) -> Self {
        use icu_properties::{
            CodePointMapData,
            props::{GeneralCategory, GeneralCategoryGroup},
        };
        let categories = CodePointMapData::<GeneralCategory>::new();
        let punctuation = |ch| GeneralCategoryGroup::Punctuation.contains(categories.get(ch));
        let excluded = |ch| {
            let category = categories.get(ch);
            if GeneralCategoryGroup::Separator.contains(category)
                || matches!(
                    category,
                    GeneralCategory::Control
                        | GeneralCategory::Format
                        | GeneralCategory::Unassigned
                )
            {
                return true;
            }
            if !punctuation(ch) {
                return false;
            }
            // CSS Text Decoration 3 keeps emphasis on punctuation whose
            // compatibility decomposition contains one of these symbols.
            !icu_normalizer::DecomposingNormalizer::new_nfkd()
                .normalize_iter(std::iter::once(ch))
                .any(|c| {
                    matches!(
                        c,
                        '#' | '%'
                            | '\u{2030}'
                            | '\u{2031}'
                            | '\u{066a}'
                            | '\u{0609}'
                            | '\u{060a}'
                            | '&'
                            | '\u{204a}'
                            | '@'
                            | '\u{00a7}'
                            | '\u{00b6}'
                            | '\u{204b}'
                            | '\u{2053}'
                            | '\u{303d}'
                    )
                })
        };
        let first = source.chars().next();
        Self {
            whitespace: first.is_some_and(char::is_whitespace),
            punctuation: first.is_some_and(punctuation),
            synthetic_hyphen,
            emphasis_excluded: synthetic_hyphen || source.chars().all(excluded),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OverlayCluster {
    pub(crate) glyphs: Range<u32>,
    pub(crate) text: Range<u32>,
}

impl<'a> GlyphRunView<'a> {
    pub fn style_index(&self) -> u32 {
        self.data().items[self.item as usize].style
    }
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

    pub fn node(&self) -> Option<NodeId> {
        self.data().items[self.item as usize].node
    }

    fn run_data(&self) -> &'a crate::shape::ShapedRun {
        match self.source {
            GlyphSource::Overlay { run: Some(run), .. } => &self.line.overlay_runs[run as usize],
            _ => &self.data().runs[self.run as usize],
        }
    }

    pub fn font(&self) -> FontId {
        self.run_data().font
    }

    /// Metrics at this run's actual size and normalized variation location.
    pub fn metrics(&self) -> crate::font::FontMetrics {
        self.run_data()
            .instance
            .metrics
            .unwrap_or_else(|| self.data().fonts.metrics(self.font(), self.font_size()))
    }

    pub fn font_size(&self) -> f32 {
        self.run_data().font_size
    }

    /// Normalized variation coordinates in the face's axis order.
    pub fn normalized_coords(&self) -> &'a [crate::font::NormalizedCoord] {
        &self.run_data().instance.coords
    }
    pub fn variations(&self) -> &'a [crate::style::FontVariation] {
        &self.run_data().instance.variations
    }
    pub fn embolden(&self) -> bool {
        self.run_data().instance.embolden
    }
    /// Synthetic slant supplied by font matching, in degrees.
    pub fn skew(&self) -> Option<f32> {
        self.run_data().instance.skew
    }
    pub fn script(&self) -> [u8; 4] {
        self.run_data().instance.script
    }
    pub fn language(&self) -> Option<&'a str> {
        self.run_data().instance.language.as_deref()
    }

    pub fn font_data(&self) -> Option<FontData> {
        self.data().fonts.font_data(self.font())
    }

    pub fn bidi_level(&self) -> u8 {
        self.record.level
    }

    pub fn text_range(&self) -> Range<usize> {
        self.text.0 as usize..self.text.1 as usize
    }

    pub fn inline_start(&self) -> f32 {
        self.record.inline_start.to_f32()
    }

    pub fn inline_size(&self) -> f32 {
        self.record.inline_size.to_f32()
    }

    /// Alphabetic baseline from the top of the line box.
    pub fn baseline(&self) -> f32 {
        (self.line.baseline + self.block_shift).to_f32()
    }

    pub fn glyphs(&self) -> Glyphs<'a> {
        Glyphs {
            view: *self,
            next: self.glyphs.0,
            end: self.glyphs.1,
        }
    }

    pub fn clusters(&self) -> impl ExactSizeIterator<Item = Cluster> + '_ {
        let data = self.data();
        let (begin, count) = match self.source {
            GlyphSource::Shared if self.glyphs.0 < self.glyphs.1 => {
                let begin = data.glyph_clusters[self.glyphs.0 as usize] as usize;
                let end = data.glyph_clusters[(self.glyphs.1 - 1) as usize] as usize;
                (begin, end + 1 - begin)
            }
            GlyphSource::Shared => (0, 0),
            GlyphSource::Overlay { clusters, .. } => {
                (clusters.0 as usize, (clusters.1 - clusters.0) as usize)
            }
        };
        (0..count).map(move |i| {
            #[cfg(test)]
            data.cluster_queries
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let (glyphs, text, store) = match self.source {
                GlyphSource::Shared => {
                    let u = &data.units[data.clusters[begin + i] as usize];
                    let crate::analysis::units::UnitKind::Cluster { glyphs, .. } = &u.kind else {
                        unreachable!()
                    };
                    (glyphs.clone(), u.shaping_text().clone(), &data.glyphs)
                }
                GlyphSource::Overlay { .. } => {
                    let c = &self.line.overlay_clusters[begin + i];
                    (
                        c.glyphs.clone(),
                        c.text.clone(),
                        self.line.overlay.as_deref().expect("overlay store"),
                    )
                }
            };
            let source = data
                .text
                .get(text.start as usize..text.end as usize)
                .unwrap_or_default();
            Cluster {
                source_char: source.chars().next(),
                flags: ClusterFlags::from_source(
                    source,
                    self.line.visible_hyphen == Some(text.start),
                ),
                text_range: text.start as usize..text.end as usize,
                advance: glyphs.clone().map(|g| self.glyph(g).advance).sum(),
                shaping_advance: glyphs.map(|g| store.advance[g as usize].to_f32()).sum(),
            }
        })
    }

    fn glyph(&self, g: u32) -> Glyph {
        let store = match self.source {
            GlyphSource::Shared => &self.data().glyphs,
            GlyphSource::Overlay { .. } => self.line.overlay.as_deref().expect("overlay store"),
        };
        let gi = g as usize;
        let first = self.glyphs.0 as usize;
        let (rel, advance) = if matches!(self.source, GlyphSource::Shared)
            && let Some((start, positions)) = &self.line.positions
        {
            let pen = positions[(g - start) as usize];
            let spacing = self
                .line
                .glyph_spacing
                .as_ref()
                .map(|(start, v)| v[(g - start) as usize]);
            let rel = pen - positions[(self.glyphs.0 - start) as usize]
                + spacing.map_or(LayoutUnit::ZERO, |s| s.leading);
            let end = if g + 1 < self.glyphs.1 {
                positions[(g + 1 - start) as usize] - positions[(self.glyphs.0 - start) as usize]
            } else {
                self.record.inline_size
            };
            (
                rel,
                spacing.map_or(end - rel, |s| store.advance[gi] + s.extra),
            )
        } else {
            let rel = store.pen[gi] - store.pen[first]
                + store.leading.as_ref().map_or(LayoutUnit::ZERO, |v| v[gi]);
            let advance = if let Some(spacing) = &store.spacing {
                store.advance[gi] + spacing[gi]
            } else if matches!(self.source, GlyphSource::Overlay { .. })
                && self.line.positions.is_some()
                && g + 1 == self.glyphs.1
            {
                self.record.inline_size - rel
            } else {
                store.advance[gi]
            };
            (rel, advance)
        };
        // Runs are stored in logical order and reversed for display here; a
        // real shaper that emits right-to-left runs in visual order must not
        // be reversed twice.
        let reversed = self.record.level % 2 != self.line.data.base_level % 2;
        let pen = if reversed {
            // Layout spacing belongs between clusters; subtracting it from
            // a glyph's ink origin would detach a mark from its base.
            self.record.inline_size - rel - store.advance[gi]
        } else {
            rel
        };
        let offset = if reversed {
            LayoutUnit::ZERO - store.offset_inline[gi]
        } else {
            store.offset_inline[gi]
        };
        let position = self.record.inline_start + pen + offset;
        Glyph {
            id: store.id[gi],
            inline_position: position.to_f32(),
            block_offset: store.offset_block[gi].to_f32(),
            advance: advance.to_f32(),
            cluster: store.cluster[gi],
        }
    }
}

/// Iterator over the glyphs of a run, with random access through `get`.
#[derive(Clone, Debug)]
pub struct Glyphs<'a> {
    view: GlyphRunView<'a>,
    next: u32,
    end: u32,
}

impl Glyphs<'_> {
    /// The `index`-th glyph of the run, independent of iteration.
    pub fn get(&self, index: usize) -> Option<Glyph> {
        let g = (self.view.glyphs.0 as usize).checked_add(index)?;
        (g < self.view.glyphs.1 as usize).then(|| self.view.glyph(g as u32))
    }
}

impl Iterator for Glyphs<'_> {
    type Item = Glyph;

    fn next(&mut self) -> Option<Glyph> {
        (self.next < self.end).then(|| {
            self.next += 1;
            self.view.glyph(self.next - 1)
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = (self.end - self.next) as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for Glyphs<'_> {}

impl Line {
    /// Fragments in visual order.
    pub fn fragments(&self) -> impl ExactSizeIterator<Item = Fragment<'_>> + '_ {
        (0..self.fragments.len()).map(move |i| self.view(i))
    }

    pub fn fragment(&self, index: usize) -> Option<Fragment<'_>> {
        (index < self.fragments.len()).then(|| self.view(index))
    }

    /// Font data of any face used by the paragraph; works without the
    /// `FontCollection`, which the line keeps alive.
    pub fn font_data(&self, id: FontId) -> Option<FontData> {
        self.data.fonts.font_data(id)
    }

    /// True when the line has no glyphs, atomic inlines, or painted inline
    /// edges. An empty line before a block has zero line advance.
    pub fn is_empty(&self) -> bool {
        self.empty
    }

    fn view(&self, index: usize) -> Fragment<'_> {
        let record = &self.fragments[index];
        let rect = |block_start: f32, block_size: f32, start: f32, size: f32| LogicalRect {
            inline_start: start,
            block_start,
            inline_size: size,
            block_size,
        };
        match &record.kind {
            RecordKind::Glyphs {
                run,
                glyphs,
                item,
                text,
                source,
            } => Fragment::GlyphRun(GlyphRunView {
                source: *source,
                block_shift: self.block_shifts[index],
                line: self,
                record,
                run: *run,
                glyphs: match source {
                    GlyphSource::Shared => (glyphs.start, glyphs.end),
                    GlyphSource::Overlay { glyphs, .. } => *glyphs,
                },
                item: *item,
                text: (text.start, text.end),
            }),
            RecordKind::Atomic { node, size, .. } => {
                let margin_block = size.margins.block_start + size.margins.block_end;
                let height = size.block_size + margin_block;
                let kind = self.data.baseline_kind(*node);
                // Missing baselines are synthesized from the margin box
                // (CSS Inline 3): bottom for alphabetic, middle for central.
                let baseline_from_top = size.baseline.unwrap_or(match kind {
                    Some(BaselineKind::Central) => height / 2.0,
                    _ => height,
                });
                let line_baseline = (self.baseline + self.block_shifts[index]).to_f32();
                let top = line_baseline - baseline_from_top;
                let start = record.inline_start.to_f32();
                let width = record.inline_size.to_f32();
                let m = size.margins;
                Fragment::Atomic(AtomicFragment {
                    node: *node,
                    margin_rect: rect(top, height, start, width),
                    border_rect: rect(
                        top + m.block_start,
                        size.block_size,
                        start + m.inline_start,
                        width - m.inline_start - m.inline_end,
                    ),
                    baseline: line_baseline,
                })
            }
            RecordKind::InlineBox {
                box_index,
                start_edge,
                end_edge,
                parent,
                reversed,
            } => {
                let info = &self.data.boxes[*box_index as usize];
                let resolved = self.data.style_metrics[info.style as usize];
                let font = resolved.font;
                let m = resolved.metrics;
                let e = info.edges;
                let pick = |on: bool, v: f32| if on { v } else { 0.0 };
                let margin_start = pick(*start_edge, e.margin.inline_start);
                let margin_end = pick(*end_edge, e.margin.inline_end);
                let inner_start = pick(*start_edge, e.border.inline_start + e.padding.inline_start);
                let inner_end = pick(*end_edge, e.border.inline_end + e.padding.inline_end);
                // A box whose direction opposes the paragraph's has its start
                // edge on the inline-end side.
                let (lead_margin, trail_margin, lead_inner, trail_inner) = if *reversed {
                    (margin_end, margin_start, inner_end, inner_start)
                } else {
                    (margin_start, margin_end, inner_start, inner_end)
                };
                let border_start = record.inline_start.to_f32() + lead_margin;
                let border_size = record.inline_size.to_f32() - lead_margin - trail_margin;
                let content_top = (self.baseline + self.block_shifts[index]).to_f32() - m.ascent;
                let content_height = m.ascent + m.descent;
                let above = e.padding.block_start + e.border.block_start;
                let below = e.padding.block_end + e.border.block_end;
                Fragment::InlineBox(InlineBoxFragment {
                    node: info.node,
                    rect: rect(
                        content_top - above,
                        content_height + above + below,
                        border_start,
                        border_size,
                    ),
                    content_rect: rect(
                        content_top,
                        content_height,
                        border_start + lead_inner,
                        border_size - lead_inner - trail_inner,
                    ),
                    has_start_edge: *start_edge,
                    has_end_edge: *end_edge,
                    parent: parent.map(|p| p as usize),
                    font,
                    font_size: resolved.size,
                })
            }
            RecordKind::Anchor { node, kind } => Fragment::OutOfFlowAnchor(AnchorFragment {
                node: *node,
                kind: *kind,
                inline_position: record.inline_start.to_f32(),
            }),
        }
    }
}
