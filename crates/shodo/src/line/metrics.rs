use super::font_metrics::StyleMetrics;
use super::fragments::{FragmentRecord, RecordKind};
use crate::analysis::units::UnitKind;
use crate::font::FontMetrics;
use crate::geometry::{BaselineKind, LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;
use crate::style::{InlineStyle, LineHeight, VerticalAlign};
use std::ops::Range;

pub(crate) struct LineMetrics {
    pub(crate) baseline: LayoutUnit,
    pub(crate) block_size: LayoutUnit,
    pub(crate) shifts: Vec<LayoutUnit>,
    pub(crate) combination_shifts: crate::hashing::FastMap<usize, LayoutUnit>,
    pub(crate) empty: bool,
}

pub(crate) fn extents(
    s: &InlineStyle,
    m: FontMetrics,
    vertical: Option<crate::font::VerticalFontMetrics>,
    upright: bool,
) -> (f32, f32) {
    let (a, d, lead) = font_extents(s, m, vertical, upright);
    (a + lead, d + lead)
}

/// Extents of an emphasized text record: `leaded` united with its marks.
///
/// Chromium 152 treats marks like ruby annotations
/// (`ComputeAnnotationOverflow`): they never enter the root or inline box
/// struts, and lines grow only where text plus marks overflows the line box.
/// A mark is half the font size and sits outside the text's em box, the
/// primary font's ascent or descent trimmed toward its normalized
/// typographic height in whole pixels. The unmarked side keeps `leaded`.
pub(crate) fn emphasized(
    data: &ParagraphData,
    style: u32,
    run_font: crate::font::FontId,
    upright: bool,
    leaded: (f32, f32),
) -> (f32, f32) {
    let s = &data.styles[style as usize];
    let Some(emphasis) = s.text_emphasis else {
        return leaded;
    };
    let metrics = data.style_metrics[style as usize];
    // Trim toward the em box of the font that set the run, which can be a
    // fallback of the primary font whose edges it starts from.
    let em_ascent = if run_font == metrics.font {
        metrics.em_ascent
    } else {
        super::font_metrics::em_ascent(&data.fonts, run_font, metrics.size)
            .unwrap_or(metrics.em_ascent)
    };
    // Blink's `AdjustTextOverUnderOffsetsForEmHeight` trims the font edge to
    // the em box in whole pixels, never more than their difference.
    let trim = |edge: f32, em: f32| edge - (edge - em).max(0.0).floor();
    let (over, under) = if upright {
        let half = metrics.size / 2.0;
        metrics.vertical_metrics.map_or((half, half), |v| {
            (trim(v.ascent, half), trim(v.descent, half))
        })
    } else {
        (
            trim(metrics.metrics.ascent, em_ascent),
            trim(metrics.metrics.descent, metrics.size - em_ascent),
        )
    };
    emphasize(data, emphasis, metrics.size, (over, under), leaded)
}

/// Adds marks of `size / 2` outside `content` on their side.
fn emphasize(
    data: &ParagraphData,
    emphasis: crate::style::TextEmphasis,
    size: f32,
    content: (f32, f32),
    leaded: (f32, f32),
) -> (f32, f32) {
    let mark = size / 2.0;
    if emphasis_over(emphasis.position, data.style.writing_mode) {
        (leaded.0.max(content.0 + mark), leaded.1)
    } else {
        (leaded.0, leaded.1.max(content.1 + mark))
    }
}

/// Whether marks sit on the line-over side (Blink
/// `ComputedStyle::GetTextEmphasisLineLogicalSide`). Horizontal text uses the
/// over or under keyword. Vertical modes put `right` on the line-over side,
/// except `sideways-lr`, whose line-over side is on the left.
pub(crate) fn emphasis_over(
    position: crate::style::TextEmphasisPosition,
    mode: crate::geometry::WritingMode,
) -> bool {
    use crate::geometry::WritingMode as W;
    use crate::style::TextEmphasisPosition as P;
    match mode {
        W::HorizontalTb => matches!(position, P::OverRight | P::OverLeft),
        W::SidewaysLr => matches!(position, P::OverLeft | P::UnderLeft),
        W::VerticalRl | W::VerticalLr | W::SidewaysRl => {
            matches!(position, P::OverRight | P::UnderRight)
        }
    }
}

/// Extents of a combined square, whose internal line-height is 1em, with the
/// emphasis marks of its text outside the square.
fn combination_extents(data: &ParagraphData, style: u32, em: f32) -> (f32, f32) {
    let half = em / 2.0;
    match data.styles[style as usize].text_emphasis {
        Some(emphasis) => emphasize(data, emphasis, em, (half, half), (half, half)),
        None => (half, half),
    }
}

/// Font ascent, descent and half-leading.
fn font_extents(
    s: &InlineStyle,
    m: FontMetrics,
    vertical: Option<crate::font::VerticalFontMetrics>,
    upright: bool,
) -> (f32, f32, f32) {
    let (a, d, gap) = if upright {
        vertical.map_or((s.font_size / 2.0, s.font_size / 2.0, 0.0), |v| {
            (v.ascent, v.descent, v.line_gap)
        })
    } else {
        (m.ascent, m.descent, m.line_gap)
    };
    let h = match s.line_height {
        LineHeight::Normal => a + d + gap,
        LineHeight::Px(v) => v,
        LineHeight::Number(n) => n * s.font_size,
    };
    let lead = (h - a - d) / 2.0;
    (a, d, lead)
}

fn shift(s: &InlineStyle, parent: StyleMetrics, parent_upright: bool, a: f32, d: f32) -> f32 {
    let (parent_over, parent_under) = if parent_upright {
        parent
            .vertical_metrics
            .map_or((parent.size / 2.0, parent.size / 2.0), |v| {
                (v.ascent, v.descent)
            })
    } else {
        (parent.metrics.ascent, parent.metrics.descent)
    };
    match s.vertical_align {
        VerticalAlign::Length(v) => -v,
        VerticalAlign::Sub => parent.metrics.subscript_offset,
        VerticalAlign::Super => -parent.metrics.superscript_offset,
        VerticalAlign::TextTop => a - parent_over,
        VerticalAlign::TextBottom => parent_under - d,
        VerticalAlign::Middle => (a - d) / 2.0 - parent.metrics.x_height / 2.0,
        _ => 0.0,
    }
}

// Return the baseline displacement and the top/bottom-aligned ancestor.
fn box_shift(
    data: &ParagraphData,
    box_: u32,
    cache: &mut crate::hashing::FastMap<u32, (f32, Option<u32>)>,
) -> (f32, Option<u32>) {
    let mut path = Vec::new();
    let mut cursor = Some(box_);
    let mut value = (0.0, None);
    while let Some(index) = cursor {
        if let Some(cached) = cache.get(&index) {
            value = *cached;
            break;
        }
        path.push(index);
        cursor = data.boxes[index as usize].parent;
    }
    for index in path.into_iter().rev() {
        let b = &data.boxes[index as usize];
        let s = &data.styles[b.style as usize];
        let parent_style = b.parent.map_or(0, |p| data.boxes[p as usize].style) as usize;
        let parent_upright = matches!(
            data.style.writing_mode,
            crate::geometry::WritingMode::VerticalRl | crate::geometry::WritingMode::VerticalLr
        ) && data.styles[parent_style].text_orientation
            != crate::style::TextOrientation::Sideways;
        let style_metrics = data.style_metrics[b.style as usize];
        let upright = matches!(
            data.style.writing_mode,
            crate::geometry::WritingMode::VerticalRl | crate::geometry::WritingMode::VerticalLr
        ) && s.text_orientation != crate::style::TextOrientation::Sideways;
        let (a, d) = extents(
            s,
            style_metrics.metrics,
            style_metrics.vertical_metrics,
            upright,
        );
        // A central inline and an alphabetic inline share the selected
        // baseline, rather than placing their different origins at zero.
        let horizontal_center =
            (style_metrics.metrics.ascent - style_metrics.metrics.descent) / 2.0;
        let dominant_shift = match (parent_upright, upright) {
            (true, false) => horizontal_center,
            (false, true) => -horizontal_center,
            _ => 0.0,
        };
        let group = value.1.or_else(|| {
            matches!(s.vertical_align, VerticalAlign::Top | VerticalAlign::Bottom).then_some(index)
        });
        value = (
            value.0
                + dominant_shift
                + shift(
                    s,
                    data.style_metrics[parent_style],
                    parent_upright,
                    a - dominant_shift,
                    d + dominant_shift,
                ),
            group,
        );
        cache.insert(index, value);
    }
    value
}

/// Resolve real record extents independently of the selected line's solver.
/// Both accepted lines and indexed ruby probes use these exact font instances.
#[derive(Debug, Default)]
pub(crate) struct ProfileResolver {
    boxes: crate::hashing::FastMap<u32, (f32, Option<u32>)>,
    parents: crate::hashing::FastMap<u32, Option<u32>>,
    atomic_styles: crate::hashing::FastMap<crate::node::NodeId, (u32, Option<u32>)>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RecordProfile {
    pub(crate) top: f32,
    pub(crate) bottom: f32,
    pub(crate) shift: f32,
    pub(crate) group: Option<u32>,
    pub(crate) own_group: Option<bool>,
}

impl ProfileResolver {
    pub(crate) fn new(data: &ParagraphData, units: Range<usize>) -> Self {
        let mut result = Self::default();
        for u in &data.units[units] {
            result.parents.insert(u.item, u.parent_box);
            if let UnitKind::Atomic { node } = u.kind {
                result.atomic_styles.insert(node, (u.item, u.parent_box));
            }
        }
        result
    }

    pub(crate) fn combination(
        &mut self,
        data: &ParagraphData,
        unit: &crate::analysis::units::Unit,
    ) -> Option<RecordProfile> {
        let index = unit.combine?;
        let span = &data.combine_spans[index as usize];
        let (base, group) = unit
            .parent_box
            .map_or((0.0, None), |b| box_shift(data, b, &mut self.boxes));
        let style = data.items[unit.item as usize].style;
        let base = base + data.combine_center_shift(style);
        let (over, under) = combination_extents(data, style, span.em);
        Some(RecordProfile {
            top: base - over,
            bottom: base + under,
            shift: base,
            group,
            own_group: None,
        })
    }

    pub(crate) fn record(
        &mut self,
        data: &ParagraphData,
        r: &FragmentRecord,
        overlay_runs: &[crate::shape::ShapedRun],
    ) -> Option<RecordProfile> {
        let (a, d, base, group, own_group) = match &r.kind {
            RecordKind::Glyphs {
                item, run, source, ..
            } => {
                let s = &data.styles[data.items[*item as usize].style as usize];
                let shaped = match source {
                    super::fragments::GlyphSource::Overlay { run: Some(run), .. } => {
                        &overlay_runs[*run as usize]
                    }
                    _ => &data.runs[*run as usize],
                };
                let metrics = shaped
                    .instance
                    .metrics
                    .unwrap_or_else(|| data.fonts.metrics(shaped.font, shaped.font_size));
                let style = data.items[*item as usize].style;
                let (a, d) =
                    if shaped.orientation == crate::shape::orientation::RunOrientation::Combined {
                        combination_extents(data, style, s.font_size)
                    } else {
                        let upright = shaped.orientation
                            == crate::shape::orientation::RunOrientation::Upright;
                        let leaded = extents(s, metrics, shaped.instance.vertical_metrics, upright);
                        emphasized(data, style, shaped.font, upright, leaded)
                    };
                let (base, group) = self
                    .parents
                    .get(item)
                    .copied()
                    .flatten()
                    .map_or((0.0, None), |b| box_shift(data, b, &mut self.boxes));
                let center_shift =
                    if shaped.orientation == crate::shape::orientation::RunOrientation::Combined {
                        data.combine_center_shift(data.items[*item as usize].style)
                    } else if shaped.orientation
                        == crate::shape::orientation::RunOrientation::SidewaysClockwise
                        && matches!(
                            data.style.writing_mode,
                            crate::geometry::WritingMode::VerticalRl
                                | crate::geometry::WritingMode::VerticalLr
                        )
                        && s.text_orientation != crate::style::TextOrientation::Sideways
                    {
                        // Mixed selects a central baseline even for its
                        // horizontally shaped, clockwise glyphs. Convert the
                        // actual fallback instance's alphabetic origin once.
                        (metrics.ascent - metrics.descent) / 2.0
                    } else {
                        0.0
                    };
                (a, d, base + center_shift, group, None)
            }
            RecordKind::InlineBox { box_index, .. } => {
                let b = &data.boxes[*box_index as usize];
                let s = &data.styles[b.style as usize];
                let style_metrics = data.style_metrics[b.style as usize];
                let upright = matches!(
                    data.style.writing_mode,
                    crate::geometry::WritingMode::VerticalRl
                        | crate::geometry::WritingMode::VerticalLr
                ) && s.text_orientation != crate::style::TextOrientation::Sideways;
                let (a, d) = extents(
                    s,
                    style_metrics.metrics,
                    style_metrics.vertical_metrics,
                    upright,
                );
                let (base, group) = box_shift(data, *box_index, &mut self.boxes);
                (a, d, base, group, None)
            }
            RecordKind::Atomic { node, size, .. } => {
                let (item, parent_box) = self.atomic_styles[node];
                let s = &data.styles[data.items[item as usize].style as usize];
                let parent_style = parent_box.map_or(0, |p| data.boxes[p as usize].style) as usize;
                let parent = data.style_metrics[parent_style];
                let parent_upright = matches!(
                    data.style.writing_mode,
                    crate::geometry::WritingMode::VerticalRl
                        | crate::geometry::WritingMode::VerticalLr
                ) && data.styles[parent_style].text_orientation
                    != crate::style::TextOrientation::Sideways;
                let (base, group) =
                    parent_box.map_or((0.0, None), |b| box_shift(data, b, &mut self.boxes));
                let height = size.block_size + size.margins.block_start + size.margins.block_end;
                let central = data.baseline_kind(*node) == Some(BaselineKind::Central);
                let baseline = size
                    .baseline
                    .unwrap_or(if central { height / 2.0 } else { height });
                let dominant_shift = if central {
                    let (over, under) = parent
                        .vertical_metrics
                        .map_or((parent.size / 2.0, parent.size / 2.0), |v| {
                            (v.ascent, v.descent)
                        });
                    -(over - under) / 2.0
                } else {
                    0.0
                };
                let own = if group.is_none() {
                    match s.vertical_align {
                        VerticalAlign::Top => Some(false),
                        VerticalAlign::Bottom => Some(true),
                        _ => None,
                    }
                } else {
                    None
                };
                // Blink grows a line for the marks of an emphasized atomic
                // inline above its margin box; under marks never reach past
                // the line box there (`ComputeAnnotationOverflow`).
                let style = data.items[item as usize].style;
                let mark = match s.text_emphasis {
                    Some(emphasis) if emphasis_over(emphasis.position, data.style.writing_mode) => {
                        data.style_metrics[style as usize].size / 2.0
                    }
                    _ => 0.0,
                };
                (
                    baseline + mark,
                    height - baseline,
                    base + dominant_shift
                        + shift(
                            s,
                            parent,
                            parent_upright,
                            baseline - dominant_shift,
                            height - baseline + dominant_shift,
                        ),
                    group,
                    own,
                )
            }
            RecordKind::Anchor { .. } => {
                return None;
            }
        };
        let top = base - a;
        let bottom = base + d;
        Some(RecordProfile {
            top,
            bottom,
            shift: base,
            group,
            own_group,
        })
    }
}

pub(crate) fn measure(
    data: &ParagraphData,
    units: Range<usize>,
    records: &[FragmentRecord],
    overlay_runs: &[crate::shape::ShapedRun],
    sat: &mut Saturation,
) -> LineMetrics {
    let root = &data.styles[0];
    let root_metrics = data.style_metrics[0];
    let root_upright = matches!(
        data.style.writing_mode,
        crate::geometry::WritingMode::VerticalRl | crate::geometry::WritingMode::VerticalLr
    ) && root.text_orientation != crate::style::TextOrientation::Sideways;
    let quirk = data
        .style
        .line_height_quirk
        .then(|| super::quirk::Struts::line(data, units.clone()));
    let root_strut = quirk.as_ref().is_none_or(|q| q.root);
    let (mut above, mut below) = if root_strut {
        extents(
            root,
            root_metrics.metrics,
            root_metrics.vertical_metrics,
            root_upright,
        )
    } else {
        // Nothing has sized the line yet; real contributions replace these.
        (f32::NEG_INFINITY, f32::NEG_INFINITY)
    };
    let mut resolver = ProfileResolver::new(data, units.clone());
    let mut empty = true;
    let mut groups: crate::hashing::FastMap<u32, (f32, f32)> = crate::hashing::FastMap::default();
    let mut ghosts: crate::hashing::FastMap<u32, (f32, f32)> = crate::hashing::FastMap::default();
    let mut combination_bases = Vec::new();
    for (offset, u) in data.units[units.clone()].iter().enumerate() {
        empty &= !matches!(u.kind, UnitKind::Tab | UnitKind::ForcedBreak);
        if let Some(index) = u.combine
            && data.combine_spans[index as usize].units.start == units.start + offset
        {
            // Measure the parent square once, including tab-only compositions.
            // Its internal line-height is 1em regardless of the inline strut.
            empty = false;
            let span = &data.combine_spans[index as usize];
            let (base, group) = u
                .parent_box
                .map_or((0.0, None), |b| box_shift(data, b, &mut resolver.boxes));
            let style = data.items[u.item as usize].style;
            let base = base + data.combine_center_shift(style);
            let (over, under) = combination_extents(data, style, span.em);
            let top = base - over;
            let bottom = base + under;
            if let Some(group) = group {
                let bounds = groups.entry(group).or_insert((top, bottom));
                bounds.0 = bounds.0.min(top);
                bounds.1 = bounds.1.max(bottom);
            } else {
                above = above.max(-top);
                below = below.max(bottom);
            }
            combination_bases.push((index as usize, base, group));
        }
    }
    let mut shifts = Vec::with_capacity(records.len());
    let mut own_groups: crate::hashing::FastMap<usize, (f32, f32, bool)> =
        crate::hashing::FastMap::default();
    let mut memberships = Vec::with_capacity(records.len());
    for (i, r) in records.iter().enumerate() {
        match &r.kind {
            RecordKind::Glyphs { .. } | RecordKind::Atomic { .. } => empty = false,
            RecordKind::InlineBox {
                box_index,
                start_edge,
                end_edge,
                ..
            } => {
                let e = data.boxes[*box_index as usize].edges;
                empty &= !((*start_edge && (e.inline_start_total() != 0.0))
                    || (*end_edge && (e.inline_end_total() != 0.0))
                    || e.border.block_start != 0.0
                    || e.border.block_end != 0.0
                    || e.padding.block_start != 0.0
                    || e.padding.block_end != 0.0);
            }
            RecordKind::Anchor { .. } => {}
        }
        let Some(profile) = resolver.record(data, r, overlay_runs) else {
            shifts.push(0.0);
            memberships.push((None, None));
            continue;
        };
        let RecordProfile {
            top,
            bottom,
            shift: base,
            group,
            own_group,
        } = profile;
        let sizes = match (&r.kind, &quirk) {
            (RecordKind::InlineBox { box_index, .. }, Some(q)) => q.contributes(Some(*box_index)),
            // A space removed at the line end sizes nothing, not even with
            // its own glyph extents (Chromium W/BI/j).
            (RecordKind::Glyphs { text, .. }, Some(q)) => !q.trimmed(data, text),
            _ => true,
        };
        if !sizes {
            if let Some(g) = group {
                // Position-only bounds for a group no member sizes.
                let v = ghosts.entry(g).or_insert((top, bottom));
                v.0 = v.0.min(top);
                v.1 = v.1.max(bottom);
            }
        } else if let Some(g) = group {
            let v = groups.entry(g).or_insert((top, bottom));
            v.0 = v.0.min(top);
            v.1 = v.1.max(bottom);
        } else if let Some(bottom_align) = own_group {
            own_groups.insert(i, (top, bottom, bottom_align));
        } else {
            above = above.max(-top);
            below = below.max(bottom);
        }
        shifts.push(base);
        memberships.push((group, own_group));
    }
    if above == f32::NEG_INFINITY {
        above = 0.0;
        below = 0.0;
    }
    // Negative half-leading is meaningful: a zero-height strut can still
    // have a positive ascent and an equally negative descent.
    let mut height = (above + below).max(0.0);
    for (top, bottom) in groups.values() {
        height = height.max(bottom - top);
    }
    for (top, bottom, _) in own_groups.values() {
        height = height.max(bottom - top);
    }
    // Bottom aligned content may require room above the root strut.
    let bottom_height = groups
        .iter()
        .filter(|(b, _)| {
            data.styles[data.boxes[**b as usize].style as usize].vertical_align
                == VerticalAlign::Bottom
        })
        .map(|(_, v)| v.1 - v.0)
        .chain(own_groups.values().filter(|v| v.2).map(|v| v.1 - v.0))
        .fold(0.0_f32, f32::max);
    if height > above + below {
        above = above.max(bottom_height - below);
    }
    let mut deltas = crate::hashing::FastMap::default();
    // Sized groups first; a ghost (position-only) group only places a group
    // that no member sized.
    for (g, (top, bottom)) in groups.into_iter().chain(ghosts) {
        if let std::collections::hash_map::Entry::Vacant(slot) = deltas.entry(g) {
            let align = data.styles[data.boxes[g as usize].style as usize].vertical_align;
            slot.insert(if align == VerticalAlign::Bottom {
                height - above - bottom
            } else {
                -above - top
            });
        }
    }
    for (i, (g, own)) in memberships.into_iter().enumerate() {
        if let Some(g) = g {
            shifts[i] += deltas[&g];
        }
        if own.is_some() {
            let (top, bottom, is_bottom) = own_groups[&i];
            shifts[i] += if is_bottom {
                height - above - bottom
            } else {
                -above - top
            };
        }
    }
    let block_size = LayoutUnit::from_f32_ceil(if empty { 0.0 } else { height }, sat);
    let over_baseline = LayoutUnit::from_f32_round(above, sat);
    // The over side is the physical right in vertical-lr, opposite block-start.
    // Reflect the line-relative solution into logical block coordinates once.
    let reverse_over = data.style.writing_mode == crate::geometry::WritingMode::VerticalLr;
    let combination_shifts = combination_bases
        .into_iter()
        .map(|(index, base, group)| {
            let displacement = base + group.map_or(0.0, |g| deltas[&g]);
            (
                index,
                LayoutUnit::from_f32_round(
                    if reverse_over {
                        -displacement
                    } else {
                        displacement
                    },
                    sat,
                ),
            )
        })
        .collect();
    LineMetrics {
        baseline: if reverse_over {
            block_size.sub(over_baseline, sat)
        } else {
            over_baseline
        },
        block_size,
        shifts: shifts
            .into_iter()
            .map(|v| LayoutUnit::from_f32_round(if reverse_over { -v } else { v }, sat))
            .collect(),
        combination_shifts,
        empty,
    }
}
