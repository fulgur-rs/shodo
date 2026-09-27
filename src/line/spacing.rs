//! Typographic character costs shared by scanning and selected placement.
use super::spacing_summary::{Cursor, Edge, Kind, Summary};
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;
use icu_properties::{
    CodePointMapData,
    props::{GeneralCategory, Script},
};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct UnitSpacing {
    pub(crate) summary: Summary,
    pub(crate) word: LayoutUnit,
    /// Last storage piece of the same source grapheme. Mark-only pieces may
    /// be split into another run when a shaping resource budget is small.
    pub(crate) tail: usize,
}

pub(crate) fn build(data: &ParagraphData, sat: &mut Saturation) -> Vec<UnitSpacing> {
    let category = CodePointMapData::<GeneralCategory>::new();
    let script = CodePointMapData::<Script>::new();
    let mut previous_text = None;
    let mut result: Vec<_> = data
        .units
        .iter()
        .enumerate()
        .map(|(index, unit)| {
            let style = &data.styles[data.items[unit.item as usize].style as usize];
            let tracking = LayoutUnit::from_f32_round(style.letter_spacing, sat).raw();
            let mut value = UnitSpacing {
                tail: index,
                ..Default::default()
            };
            match unit.kind {
                UnitKind::Cluster { .. } => {
                    if previous_text.as_ref() == Some(&unit.text) {
                        return value;
                    }
                    previous_text = Some(unit.text.clone());
                    let start = data
                        .breaks
                        .graphemes
                        .partition_point(|g| *g < unit.text.start);
                    let end = data
                        .breaks
                        .graphemes
                        .partition_point(|g| *g < unit.text.end);
                    for &offset in &data.breaks.graphemes[start..end] {
                        let Some(ch) = data.text[offset as usize..].chars().next() else {
                            continue;
                        };
                        if matches!(
                            category.get(ch),
                            GeneralCategory::Format | GeneralCategory::Control
                        ) {
                            continue;
                        }
                        let cursive = matches!(
                            script.get(ch),
                            Script::Arabic
                                | Script::HanifiRohingya
                                | Script::Mandaic
                                | Script::Mongolian
                                | Script::Nko
                                | Script::PhagsPa
                                | Script::Syriac
                        ) && matches!(
                            category.get(ch),
                            GeneralCategory::UppercaseLetter
                                | GeneralCategory::LowercaseLetter
                                | GeneralCategory::TitlecaseLetter
                                | GeneralCategory::ModifierLetter
                                | GeneralCategory::OtherLetter
                        );
                        value.summary = value.summary.join(Summary::leaf(Edge {
                            tracking,
                            kind: if cursive { Kind::Cursive } else { Kind::Text },
                        }));
                        if matches!(
                            ch,
                            ' ' | '\u{a0}'
                                | '\u{1361}'
                                | '\u{10100}'
                                | '\u{10101}'
                                | '\u{1039f}'
                                | '\u{1091f}'
                        ) {
                            value.word = value
                                .word
                                .add(LayoutUnit::from_f32_round(style.word_spacing, sat), sat);
                        }
                    }
                }
                UnitKind::Atomic { .. } => {
                    value.summary = Summary::leaf(Edge {
                        tracking,
                        kind: Kind::Atomic,
                    })
                }
                UnitKind::Tab | UnitKind::ForcedBreak | UnitKind::BlockInInline { .. } => {
                    value.summary = Summary::leaf(Edge {
                        tracking: 0,
                        kind: Kind::Barrier,
                    })
                }
                _ => {}
            }
            value
        })
        .collect();
    let mut owner: Option<usize> = None;
    for (i, unit) in data.units.iter().enumerate() {
        if !matches!(unit.kind, UnitKind::Cluster { .. }) {
            continue;
        }
        if let Some(first) = owner
            && data
                .breaks
                .graphemes
                .partition_point(|g| *g < data.units[first].text.end)
                == data
                    .breaks
                    .graphemes
                    .partition_point(|g| *g <= unit.text.start)
        {
            result[first].tail = i;
        } else {
            owner = Some(i);
        }
    }
    result
}

pub(super) fn push(data: &ParagraphData, cursor: &mut Cursor, index: usize) {
    let unit = &data.units[index];
    let level = if matches!(unit.kind, UnitKind::Tab) {
        data.base_level
    } else {
        unit.level
    };
    cursor.push(level, data.unit_spacing[index].summary);
}

pub(super) fn intercharacter_allowed(data: &ParagraphData, left: usize, right: usize) -> bool {
    !data.unit_spacing[left]
        .summary
        .last
        .zip(data.unit_spacing[right].summary.first)
        .is_some_and(|(a, b)| a.kind == Kind::Cursive && b.kind == Kind::Cursive)
}

pub(super) fn hyphen(
    data: &ParagraphData,
    cursor: &Cursor,
    index: usize,
    sat: &mut Saturation,
) -> LayoutUnit {
    let mut cursor = cursor.clone();
    let unit = &data.units[index];
    let style = &data.styles[data.items[unit.item as usize].style as usize];
    cursor.push(
        unit.level,
        Summary::leaf(Edge {
            tracking: LayoutUnit::from_f32_round(style.letter_spacing, sat).raw(),
            kind: Kind::Text,
        }),
    );
    cursor.summary().width(sat)
}

pub(super) fn width(
    data: &ParagraphData,
    start: usize,
    end: usize,
    visible_hyphen: Option<u32>,
    sat: &mut Saturation,
) -> LayoutUnit {
    let mut cursor = Cursor::default();
    for i in start..end {
        if visible_hyphen == Some(data.units[i].text.start)
            && data.text[data.units[i].text.start as usize..].starts_with('\u{ad}')
        {
            let unit = &data.units[i];
            let style = &data.styles[data.items[unit.item as usize].style as usize];
            cursor.push(
                unit.level,
                Summary::leaf(Edge {
                    tracking: LayoutUnit::from_f32_round(style.letter_spacing, sat).raw(),
                    kind: Kind::Text,
                }),
            );
        } else {
            push(data, &mut cursor, i);
        }
    }
    cursor.summary().width(sat)
}

/// Allocate gaps only after one visual reorder of the selected line.
/// Before-pens belong to RTL clusters so added advance never moves their ink
/// away from attached marks or introduces spacing at the visual line edge.
pub(super) fn apply(
    data: &ParagraphData,
    start: usize,
    scan: &mut super::Scan,
    sat: &mut Saturation,
) {
    if data
        .styles
        .iter()
        .all(|s| s.letter_spacing == 0.0 && s.word_spacing == 0.0)
    {
        return;
    }
    let mut leading: Vec<_> = data.unit_spacing[start..scan.end]
        .iter()
        .map(|u| u.word.div_i32(2))
        .collect();
    let visible = scan
        .overlays
        .iter()
        .find_map(|w| w.hyphen.as_ref().map(|t| t.start));
    let mut metadata: Vec<_> = data.unit_spacing[start..scan.end]
        .iter()
        .map(|u| u.summary)
        .collect();
    for (k, unit) in data.units[start..scan.end].iter().enumerate() {
        let tail = data.unit_spacing[start + k].tail.min(scan.end - 1);
        if tail > start + k {
            let after = data.unit_spacing[start + k].word.sub(leading[k], sat);
            scan.widths[k] = scan.widths[k].sub(after, sat);
            scan.widths[tail - start] = scan.widths[tail - start].add(after, sat);
        }
        if visible == Some(unit.text.start)
            && data.text[unit.text.start as usize..].starts_with('\u{ad}')
        {
            let style = &data.styles[data.items[unit.item as usize].style as usize];
            metadata[k] = Summary::leaf(Edge {
                tracking: LayoutUnit::from_f32_round(style.letter_spacing, sat).raw(),
                kind: Kind::Text,
            });
        }
        scan.widths[k] =
            scan.widths[k].add(super::spacing_summary::raw(metadata[k].cost, sat), sat);
    }
    let mut previous: Option<(usize, Edge)> = None;
    for range in [start..scan.hang_start, scan.hang_start..scan.end] {
        let levels: Vec<_> = range
            .clone()
            .map(|i| {
                unicode_bidi::Level::new(
                    if i >= scan.hang_start || matches!(data.units[i].kind, UnitKind::Tab) {
                        data.base_level
                    } else {
                        data.units[i].level
                    },
                )
                .unwrap()
            })
            .collect();
        let mut order = unicode_bidi::BidiInfo::reorder_visual(&levels);
        if data.base_level % 2 == 1 {
            order.reverse();
        }
        for offset in order {
            let i = range.start + offset;
            let summary = metadata[i - start];
            let reversed = data.units[i].level % 2 != data.base_level % 2;
            let (first, last) = if reversed {
                (summary.last, summary.first)
            } else {
                (summary.first, summary.last)
            };
            let Some(first) = first else {
                continue;
            };
            if let Some((j, edge)) = previous {
                let cost = super::spacing_summary::gap(edge, first);
                let gap = super::spacing_summary::raw(cost, sat);
                // Hanging whitespace takes the content→space gap itself;
                // the content width remains the trimmed candidate width.
                if i >= scan.hang_start && j < scan.hang_start {
                    scan.widths[i - start] = scan.widths[i - start].add(gap, sat);
                    leading[i - start] = leading[i - start].add(gap, sat);
                } else if super::spacing_summary::allowed(edge, first) {
                    let left = i64::from(edge.tracking) / 2;
                    let right = cost - left;
                    for (unit, extra, before) in [
                        (j, left, data.units[j].level % 2 != data.base_level % 2),
                        (i, right, !reversed),
                    ] {
                        let target = if before {
                            unit
                        } else {
                            data.unit_spacing[unit].tail.min(scan.end - 1)
                        };
                        let extra = super::spacing_summary::raw(extra, sat);
                        scan.widths[target - start] = scan.widths[target - start].add(extra, sat);
                        if before {
                            leading[target - start] = leading[target - start].add(extra, sat);
                        }
                    }
                }
            }
            previous = Some((i, last.unwrap()));
        }
    }
    scan.leading = Some(leading);
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GlyphSpacing {
    pub(crate) leading: LayoutUnit,
    pub(crate) extra: LayoutUnit,
}

pub(super) type Positions = (u32, Vec<LayoutUnit>);
pub(super) type Adjustments = (u32, Vec<GlyphSpacing>);

/// Build pens once after tracking and justification, from the final advances.
pub(super) fn positions(
    data: &ParagraphData,
    start: usize,
    scan: &mut super::Scan,
    justified: bool,
    sat: &mut Saturation,
) -> (Option<Positions>, Option<Adjustments>) {
    let leading = scan.leading.as_deref();
    if leading.is_none() && !justified {
        return (None, None);
    }
    let mut prefix = vec![LayoutUnit::ZERO];
    for w in &scan.widths {
        prefix.push(prefix.last().unwrap().add(*w, sat));
    }
    let clusters: Vec<_> = (start..scan.end)
        .filter(|i| matches!(data.units[*i].kind, UnitKind::Cluster { .. }))
        .collect();
    let glyph_range = clusters.first().zip(clusters.last()).map(|(a, b)| {
        let UnitKind::Cluster { glyphs: a, .. } = &data.units[*a].kind else {
            unreachable!()
        };
        let UnitKind::Cluster { glyphs: b, .. } = &data.units[*b].kind else {
            unreachable!()
        };
        a.start..b.end
    });
    let Some(range) = glyph_range else {
        return (None, None);
    };
    let mut points = vec![LayoutUnit::ZERO; (range.end - range.start) as usize];
    let mut adjustments = vec![GlyphSpacing::default(); points.len()];
    let mut cursor = 0;
    while cursor < clusters.len() {
        let i = clusters[cursor];
        let unit = &data.units[i];
        let UnitKind::Cluster { glyphs, .. } = &unit.kind else {
            unreachable!()
        };
        let mut end = cursor + 1;
        while end < clusters.len() && unit.shares_cluster(&data.units[clusters[end]]) {
            end += 1;
        }
        let mut width = LayoutUnit::ZERO;
        let mut before = LayoutUnit::ZERO;
        for index in &clusters[cursor..end] {
            width = width.add(scan.widths[*index - start], sat);
            before = before.add(leading.map_or(LayoutUnit::ZERO, |v| v[*index - start]), sat);
        }
        let natural = glyphs.clone().fold(LayoutUnit::ZERO, |p, g| {
            p.add(data.glyphs.advance[g as usize], sat)
        });
        for g in glyphs.clone() {
            let index = (g - range.start) as usize;
            points[index] = prefix[i - start].add(
                data.glyphs.pen[g as usize].sub(data.glyphs.pen[glyphs.start as usize], sat),
                sat,
            );
            adjustments[index].leading = before;
        }
        adjustments[(glyphs.end - 1 - range.start) as usize].extra = width.sub(natural, sat);
        cursor = end;
    }
    // Reconcile each owned cluster against the unit contributions produced by
    // the same window. Several source slices can share one actual glyph.
    for window in &mut scan.overlays {
        let owners: Vec<_> = clusters
            .iter()
            .copied()
            .filter(|i| {
                window.text.start <= data.units[*i].text.start
                    && data.units[*i].text.start < window.text.end
            })
            .collect();
        if owners.is_empty() {
            continue;
        }
        let mut by_unit = vec![Vec::new(); owners.len()];
        for (g, text) in window.store.cluster.iter().enumerate() {
            let owner = owners
                .partition_point(|i| data.units[*i].text.start <= *text)
                .saturating_sub(1);
            by_unit[owner].push(g);
        }
        let mut extra = window
            .store
            .spacing
            .clone()
            .unwrap_or_else(|| vec![LayoutUnit::ZERO; window.store.len()]);
        let mut before = vec![LayoutUnit::ZERO; window.store.len()];
        let mut previous = None;
        for (owner, gs) in by_unit.iter().enumerate() {
            let index = owners[owner];
            if let Some(&last) = gs.last() {
                previous = Some(last);
            }
            let Some(last) = previous else {
                continue;
            };
            let natural = gs.iter().fold(LayoutUnit::ZERO, |p, g| {
                p.add(window.store.advance[*g], sat)
            });
            let allocated = gs
                .iter()
                .fold(LayoutUnit::ZERO, |p, g| p.add(extra[*g], sat));
            extra[last] = extra[last].add(
                scan.widths[index - start]
                    .sub(natural, sat)
                    .sub(allocated, sat),
                sat,
            );
            let cluster = window.store.cluster[last];
            let first = window.store.cluster[..=last].partition_point(|c| *c < cluster);
            for value in &mut before[gs.first().copied().unwrap_or(first)..=last] {
                *value = value.add(leading.map_or(LayoutUnit::ZERO, |v| v[index - start]), sat);
            }
        }
        for run in &window.runs {
            let mut shift = LayoutUnit::ZERO;
            for g in run.glyphs.clone() {
                let g = g as usize;
                window.store.pen[g] = window.store.pen[g].add(shift, sat);
                shift = shift.add(extra[g], sat);
            }
        }
        window.store.spacing = Some(extra);
        window.store.leading = Some(before);
    }
    (
        Some((range.start, points)),
        Some((range.start, adjustments)),
    )
}
