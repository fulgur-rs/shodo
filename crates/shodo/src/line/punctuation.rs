//! Punctuation classification and blank space from the actual shaping face.
use crate::geometry::{LayoutUnit, Saturation, WritingMode};
use crate::paragraph::ParagraphData;
use crate::style::TextSpacingTrim;
use icu_properties::{
    CodePointMapData,
    props::{BidiMirroringGlyph, EastAsianWidth, GeneralCategory},
};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::{LocationRef, Size},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PunctuationClass {
    #[default]
    Other,
    Opening,
    Closing,
    Middle,
    Space,
    Ps,
    Pe,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Punctuation {
    pub(crate) source: u32,
    pub(crate) class: PunctuationClass,
    pub(crate) size: f32,
    pub(crate) trim: TextSpacingTrim,
    pub(crate) left: LayoutUnit,
    pub(crate) right: LayoutUnit,
    pub(crate) advance: LayoutUnit,
    pub(crate) first: bool,
    pub(crate) last: bool,
    pub(crate) stop: bool,
}

impl Punctuation {
    pub(super) fn own_blanks(self) -> (LayoutUnit, LayoutUnit) {
        if self.trim == TextSpacingTrim::TrimAll {
            (self.left, self.right)
        } else {
            (LayoutUnit::ZERO, LayoutUnit::ZERO)
        }
    }
    fn layout_advance(self, sat: &mut Saturation) -> LayoutUnit {
        let (left, right) = self.own_blanks();
        self.advance
            .sub(left, sat)
            .sub(right, sat)
            .max(LayoutUnit::ZERO)
    }
}

pub(crate) fn classify(ch: char, language: Option<&str>) -> PunctuationClass {
    use PunctuationClass as P;
    let gc = CodePointMapData::<GeneralCategory>::new().get(ch);
    let full = ('\u{3000}'..='\u{303f}').contains(&ch)
        || CodePointMapData::<EastAsianWidth>::new().get(ch) == EastAsianWidth::Fullwidth;
    match ch {
        '\u{3000}' => P::Space,
        '\u{2018}' | '\u{201c}' => P::Opening,
        '\u{2019}' | '\u{201d}' => P::Closing,
        '\u{00b7}' | '\u{2027}' | '\u{30fb}' => P::Middle,
        '\u{ff1a}' | '\u{ff1b}' => {
            if chinese_traditional(language) == Some(false) {
                P::Closing
            } else {
                P::Middle
            }
        }
        '\u{3001}' | '\u{3002}' | '\u{ff0c}' | '\u{ff0e}' => {
            if chinese_traditional(language) == Some(true) {
                P::Middle
            } else {
                P::Closing
            }
        }
        _ => match gc {
            GeneralCategory::OpenPunctuation if full => P::Opening,
            GeneralCategory::ClosePunctuation if full => P::Closing,
            GeneralCategory::OpenPunctuation => P::Ps,
            GeneralCategory::ClosePunctuation => P::Pe,
            _ => P::Other,
        },
    }
}

/// The explicit script takes precedence over the region. Read only the
/// language/script/region prefix; extensions and private use are not regions.
fn chinese_traditional(language: Option<&str>) -> Option<bool> {
    let mut subtags = language?.split('-');
    if !subtags.next()?.eq_ignore_ascii_case("zh") {
        return None;
    }
    let Some(second) = subtags.next() else {
        return Some(false);
    };
    if second.eq_ignore_ascii_case("Hant") {
        return Some(true);
    }
    if second.eq_ignore_ascii_case("Hans") {
        return Some(false);
    }
    let region = if second.len() == 4 {
        subtags.next().unwrap_or("")
    } else {
        second
    };
    Some(
        ["TW", "HK", "MO"]
            .iter()
            .any(|r| region.eq_ignore_ascii_case(r)),
    )
}

pub(super) fn last_edge(data: &ParagraphData, end: usize) -> bool {
    data.last_content_unit.is_some_and(|i| i < end)
}

/// One entry per typographic start, so compressed/shared storage pieces do
/// not substitute their first character's punctuation class for every glyph.
pub(crate) fn build(data: &ParagraphData, sat: &mut Saturation) -> Vec<Punctuation> {
    let mut item = 0;
    let mut shaping = 0;
    let mut result: Vec<_> = data
        .breaks
        .typographic_starts
        .iter()
        .map(|&offset| {
            while data.items.get(item).is_some_and(|i| i.text.end <= offset) {
                item += 1;
            }
            let Some(source) = data.items.get(item) else {
                return Punctuation::default();
            };
            let style = &data.styles[source.style as usize];
            let ch = data.text[offset as usize..].chars().next().unwrap_or('\0');
            let gc = CodePointMapData::<GeneralCategory>::new().get(ch);
            while data
                .shape_items
                .get(shaping)
                .is_some_and(|i| i.end <= offset)
            {
                shaping += 1;
            }
            let level = data
                .shape_items
                .get(shaping)
                .map_or(data.base_level, |i| i.level);
            // Shaping mirrors brackets in odd embedding levels. Classification
            // and safe blanks describe the physical glyph, not its source name.
            let shaped = if level % 2 == 1 {
                CodePointMapData::<BidiMirroringGlyph>::new()
                    .get(ch)
                    .mirroring_glyph
                    .unwrap_or(ch)
            } else {
                ch
            };
            Punctuation {
                source: offset,
                class: classify(shaped, style.lang.as_deref()),
                size: style.font_size,
                trim: style.text_spacing_trim,
                first: matches!(
                    gc,
                    GeneralCategory::OpenPunctuation
                        | GeneralCategory::InitialPunctuation
                        | GeneralCategory::FinalPunctuation
                ) || matches!(ch, '\'' | '"' | '\u{3000}'),
                last: matches!(
                    gc,
                    GeneralCategory::ClosePunctuation
                        | GeneralCategory::InitialPunctuation
                        | GeneralCategory::FinalPunctuation
                ) || matches!(ch, '\'' | '"'),
                stop: matches!(
                    ch,
                    ',' | '.'
                        | '\u{060c}'
                        | '\u{06d4}'
                        | '\u{3001}'
                        | '\u{3002}'
                        | '\u{ff0c}'
                        | '\u{ff0e}'
                        | '\u{fe50}'
                        | '\u{fe51}'
                        | '\u{fe52}'
                        | '\u{ff61}'
                        | '\u{ff64}'
                ),
                ..Default::default()
            }
        })
        .collect();
    // `typographic_starts` is sorted, so a binary search replaces the
    // offset -> index map (no hashing, no extra allocation); the last equal
    // start wins, as it did when the map was collected.
    let starts = &data.breaks.typographic_starts;
    let start_index = |offset: u32| {
        let after = starts.partition_point(|s| *s <= offset);
        after.checked_sub(1).filter(|i| starts[*i] == offset)
    };
    for (g, &offset) in data.glyphs.cluster.iter().enumerate() {
        if let Some(index) = start_index(offset) {
            result[index].advance = result[index].advance.add(data.glyphs.advance[g], sat);
        }
    }
    let mut shaping = 0;
    for run in &data.runs {
        while data
            .shape_items
            .get(shaping)
            .is_some_and(|i| i.end <= run.text.start)
        {
            shaping += 1;
        }
        let level = data
            .shape_items
            .get(shaping)
            .map_or(data.base_level, |i| i.level);
        let Some(blob) = data.fonts.font_data(run.font) else {
            continue;
        };
        let Ok(font) = FontRef::from_index(blob.data.as_ref(), blob.index) else {
            continue;
        };
        let metrics = font.glyph_metrics(
            Size::new(run.font_size),
            LocationRef::new(&run.instance.coords),
        );
        let charmap = font.charmap();
        let nominal_metric = charmap.map('水').and_then(|g| metrics.advance_width(g));
        let nominal = nominal_metric.unwrap_or(run.font_size);
        // A proportional ideograph face cannot supply a dependable fullwidth
        // measure. Only compare characters that the retained face contains.
        let fullwidth_probes =
            ['卜', '一'].map(|ch| charmap.map(ch).and_then(|g| metrics.advance_width(g)));
        let proportional = fullwidth_probes
            .iter()
            .flatten()
            .any(|width| (*width - nominal).abs() > 1.0 / 64.0);
        let reliable_fullwidth_metric = nominal_metric.is_some() && !proportional;
        if proportional || run.instance.embolden || run.instance.skew.is_some() {
            continue;
        }
        let mut g = run.glyphs.start as usize;
        while g < run.glyphs.end as usize {
            let begin = g;
            let offset = data.glyphs.cluster[g];
            while g < run.glyphs.end as usize && data.glyphs.cluster[g] == offset {
                g += 1;
            }
            let Some(index) = start_index(offset) else {
                continue;
            };
            let p = &mut result[index];
            let source_char = data.text[offset as usize..].chars().next().unwrap_or('\0');
            if data.style.writing_mode == WritingMode::HorizontalTb
                && p.class == PunctuationClass::Closing
                && matches!(source_char, '\u{2019}' | '\u{201d}')
                && g == begin + 1
                // Pen-budget splits may place one cluster across multiple runs.
                && (begin == 0 || data.glyphs.cluster[begin - 1] != offset)
                && (g == data.glyphs.cluster.len() || data.glyphs.cluster[g] != offset)
                && reliable_fullwidth_metric
                && !run.instance.embolden
                && run.instance.skew.is_none()
            {
                // Use the selected glyph's nominal metric: shaping features
                // such as author-provided `halt` may change its advance.
                if let (Some(nominal), Some(glyph)) = (
                    nominal_metric,
                    metrics.advance_width(GlyphId::new(data.glyphs.id[begin])),
                ) && glyph < nominal - 1.0 / 64.0
                {
                    p.class = PunctuationClass::Pe;
                }
            }
            if !matches!(
                p.class,
                PunctuationClass::Opening | PunctuationClass::Closing | PunctuationClass::Middle
            ) {
                continue;
            }
            let advance = data.glyphs.advance[begin..g]
                .iter()
                .fold(LayoutUnit::ZERO, |w, a| w.add(*a, sat));
            if (advance.to_f32() - nominal).abs() > 1.0 / 64.0 {
                continue;
            }
            let mut bounds: Option<(f32, f32)> = None;
            for glyph in begin..g {
                if let Some(b) = metrics.bounds(GlyphId::new(data.glyphs.id[glyph])) {
                    let pen = data.glyphs.pen[glyph]
                        .sub(data.glyphs.pen[begin], sat)
                        .add(data.glyphs.offset_inline[glyph], sat);
                    let pen = if level % 2 == 1 {
                        advance.sub(pen, sat).sub(data.glyphs.advance[glyph], sat)
                    } else {
                        pen
                    }
                    .to_f32();
                    bounds = Some(bounds.map_or((pen + b.x_min, pen + b.x_max), |(l, r)| {
                        (l.min(pen + b.x_min), r.max(pen + b.x_max))
                    }));
                }
            }
            let Some((left, right)) = bounds else {
                continue;
            };
            let half = advance.div_i32(2);
            match p.class {
                PunctuationClass::Opening if left >= half.to_f32() => p.left = half,
                PunctuationClass::Closing if advance.to_f32() - right >= half.to_f32() => {
                    p.right = half
                }
                PunctuationClass::Middle => {
                    let quarter = half.div_i32(2);
                    if left >= quarter.to_f32() && advance.to_f32() - right >= quarter.to_f32() {
                        p.left = quarter;
                        p.right = quarter;
                    }
                }
                _ => {}
            }
        }
    }
    result
}

pub(super) fn pair(a: Punctuation, b: Punctuation) -> (LayoutUnit, LayoutUnit) {
    use PunctuationClass as P;
    let left = if !matches!(b.trim, TextSpacingTrim::SpaceAll | TextSpacingTrim::TrimAll)
        && b.class == P::Opening
        && (matches!(a.class, P::Opening | P::Middle | P::Space | P::Ps)
            || a.class == P::Closing && a.size >= b.size)
    {
        b.left
    } else {
        LayoutUnit::ZERO
    };
    let right = if !matches!(a.trim, TextSpacingTrim::SpaceAll | TextSpacingTrim::TrimAll)
        && a.class == P::Closing
        && (matches!(b.class, P::Closing | P::Middle | P::Space | P::Pe)
            || b.class == P::Opening && b.size > a.size)
    {
        a.right
    } else {
        LayoutUnit::ZERO
    };
    (right, left)
}

pub(super) fn boundary(
    data: &ParagraphData,
    a: super::spacing_summary::Edge,
    b: super::spacing_summary::Edge,
    blocked: bool,
) -> (LayoutUnit, LayoutUnit) {
    // Most neighbors are ordinary letters whose pair adjustment is zero
    // either way, so decide that before consulting the spacing tree.
    let adjustment = pair(a.punctuation(data), b.punctuation(data));
    if adjustment == (LayoutUnit::ZERO, LayoutUnit::ZERO)
        || blocked
        || !super::spacing_summary::allowed(a, b)
        || !data
            .spacing_tree
            .unobstructed(a.box_node as usize, b.box_node as usize)
    {
        return (LayoutUnit::ZERO, LayoutUnit::ZERO);
    }
    adjustment
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EdgeAdjustment {
    pub(super) start_trim: LayoutUnit,
    pub(super) end_trim: LayoutUnit,
    pub(crate) hang_start: LayoutUnit,
    pub(super) hang_end: LayoutUnit,
    start_unit: Option<u32>,
    end_unit: Option<u32>,
}

impl EdgeAdjustment {
    /// Width taken off the line by the edges. Every component is clamped to
    /// be non-negative when computed, so an adjustment never widens a line.
    pub(super) fn removed(self, sat: &mut Saturation) -> LayoutUnit {
        self.start_trim
            .add(self.end_trim, sat)
            .add(self.hang_start, sat)
            .add(self.hang_end, sat)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn edges(
    data: &ParagraphData,
    summary: super::spacing_summary::Summary,
    flags: u8,
    last: bool,
    options: &crate::style::LineOptions,
    available: LayoutUnit,
    natural: LayoutUnit,
    sat: &mut Saturation,
) -> EdgeAdjustment {
    use crate::paragraph::BreakToken;
    let mut value = EdgeAdjustment::default();
    let ltr = data.base_level.is_multiple_of(2);
    let (first, end) = if ltr {
        (summary.first, summary.last)
    } else {
        (summary.last, summary.first)
    };
    if let Some(first) = first {
        value.start_unit = Some(first.unit);
        let p = first.punctuation(data);
        let blocked = (if ltr {
            summary.hang_before
        } else {
            summary.hang_after
        }) || !data
            .spacing_tree
            .cloned_outer_clear(first.box_node as usize, data.base_level.is_multiple_of(2));
        if !blocked {
            if p.class
                == if ltr {
                    PunctuationClass::Opening
                } else {
                    PunctuationClass::Closing
                }
                && (matches!(
                    p.trim,
                    TextSpacingTrim::TrimStart | TextSpacingTrim::TrimBoth | TextSpacingTrim::Auto
                ) || p.trim == TextSpacingTrim::SpaceFirst
                    && flags & (BreakToken::FIRST_LINE | BreakToken::AFTER_FORCED) == 0)
            {
                value.start_trim = (if ltr { p.left } else { p.right }).max(LayoutUnit::ZERO);
            }
            if flags & BreakToken::FIRST_LINE != 0 && options.hanging_punctuation.first && p.first {
                value.hang_start = p
                    .layout_advance(sat)
                    .sub(value.start_trim, sat)
                    .max(LayoutUnit::ZERO);
            }
        }
    }
    if let Some(end) = end {
        value.end_unit = Some(end.unit);
        let p = end.punctuation(data);
        let blocked = (if ltr {
            summary.hang_after
        } else {
            summary.hang_before
        }) || !data
            .spacing_tree
            .cloned_outer_clear(end.box_node as usize, !data.base_level.is_multiple_of(2));
        if !blocked {
            let need = natural
                .sub(value.start_trim, sat)
                .sub(value.hang_start, sat)
                .sub(available, sat);
            // Storage units may contain several typographic characters. Only
            // deductions on this very source character share its advance.
            let start_removed = first
                .filter(|first| first.punctuation(data).source == p.source)
                .map_or(LayoutUnit::ZERO, |_| {
                    value.start_trim.add(value.hang_start, sat)
                });
            let remaining = p
                .layout_advance(sat)
                .sub(start_removed, sat)
                .max(LayoutUnit::ZERO);
            if p.class
                == if ltr {
                    PunctuationClass::Closing
                } else {
                    PunctuationClass::Opening
                }
                && (matches!(p.trim, TextSpacingTrim::TrimBoth | TextSpacingTrim::Auto)
                    || matches!(
                        p.trim,
                        TextSpacingTrim::Normal
                            | TextSpacingTrim::TrimStart
                            | TextSpacingTrim::SpaceFirst
                    ) && need > LayoutUnit::ZERO)
            {
                value.end_trim = (if ltr { p.right } else { p.left })
                    .max(LayoutUnit::ZERO)
                    .min(remaining);
            }
            let advance = remaining.sub(value.end_trim, sat).max(LayoutUnit::ZERO);
            if options.hanging_punctuation.force_end && p.stop
                || last && options.hanging_punctuation.last && p.last
            {
                value.hang_end = advance;
            } else if options.hanging_punctuation.allow_end && p.stop {
                value.hang_end = need
                    .sub(value.end_trim, sat)
                    .max(LayoutUnit::ZERO)
                    .min(advance);
            }
        }
    }
    value
}

/// Width that punctuation trimming and hanging take off the edges of the
/// line summarized by `spacing`, whose natural width is `natural`.
pub(super) fn removed<'a>(
    data: &'a ParagraphData,
    spacing: &mut super::spacing_summary::Cursor<'a>,
    flags: u8,
    last: bool,
    options: &crate::style::LineOptions,
    natural: LayoutUnit,
    sat: &mut Saturation,
) -> LayoutUnit {
    edges(
        data,
        spacing.summary(Some(data)),
        flags,
        last,
        options,
        LayoutUnit::ZERO,
        natural,
        sat,
    )
    .removed(sat)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    data: &ParagraphData,
    start: usize,
    scan: &mut super::Scan,
    flags: u8,
    options: &crate::style::LineOptions,
    available: LayoutUnit,
    indent: LayoutUnit,
    sat: &mut Saturation,
) {
    let mut cursor = super::spacing_summary::Cursor::default();
    for i in start..scan.hang_start {
        super::spacing::push(data, &mut cursor, i);
    }
    scan.punctuation_edges = edges(
        data,
        cursor.summary(Some(data)),
        flags,
        last_edge(data, scan.end),
        options,
        available,
        scan.content.add(indent, sat),
        sat,
    );
    scan.content = scan.content.sub(scan.punctuation_edges.removed(sat), sat);
}

pub(super) fn apply(
    data: &ParagraphData,
    start: usize,
    scan: &mut super::Scan,
    sat: &mut Saturation,
) {
    let e = scan.punctuation_edges;
    if e.start_trim == LayoutUnit::ZERO && e.end_trim == LayoutUnit::ZERO {
        return;
    }
    let leading = scan
        .leading
        .get_or_insert_with(|| vec![LayoutUnit::ZERO; scan.end - start]);
    for (unit, amount, at_start) in [
        (e.start_unit, e.start_trim, true),
        (e.end_unit, e.end_trim, false),
    ] {
        if let Some(unit) = unit {
            let unit = unit as usize;
            scan.widths[unit - start] = scan.widths[unit - start].sub(amount, sat);
            let reversed = data.units[unit].level % 2 != data.base_level % 2;
            if at_start != reversed {
                leading[unit - start] = leading[unit - start].sub(amount, sat);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn intrinsic(
    data: &ParagraphData,
    summary: super::spacing_summary::Summary,
    flags: u8,
    last: bool,
    options: &crate::style::LineOptions,
    natural: LayoutUnit,
    minimum: bool,
    sat: &mut Saturation,
) -> LayoutUnit {
    let adjustment = edges(
        data,
        summary,
        flags,
        last,
        options,
        if minimum {
            LayoutUnit::ZERO
        } else {
            LayoutUnit::MAX
        },
        natural,
        sat,
    );
    natural.sub(adjustment.removed(sat), sat)
}

/// JLREQ expansion exclusions are independent of line-break prohibitions.
/// Source offsets also work inside an owned or compressed shaping cluster.
pub(super) fn justify_boundary(data: &ParagraphData, left: u32, right: u32) -> bool {
    use icu_properties::props::{LineBreak, Script};
    let profile = |offset: u32| {
        if data.combine_at_text(offset).is_some() {
            return ('\u{fffc}', false, false);
        }
        let ch = data.text[offset as usize..].chars().next().unwrap_or('\0');
        let item = data.items.partition_point(|i| i.text.end <= offset);
        let language = data
            .items
            .get(item)
            .and_then(|i| data.styles[i.style as usize].lang.as_deref());
        let japanese = language.map_or_else(
            || {
                matches!(
                    CodePointMapData::<Script>::new().get(ch),
                    Script::Han | Script::Hiragana | Script::Katakana
                ) || ('\u{3000}'..='\u{303f}').contains(&ch)
                    || CodePointMapData::<EastAsianWidth>::new().get(ch)
                        == EastAsianWidth::Fullwidth
            },
            |lang| {
                lang.split('-')
                    .next()
                    .is_some_and(|l| l.eq_ignore_ascii_case("ja"))
            },
        );
        let protected = matches!(
            classify(ch, language),
            PunctuationClass::Opening
                | PunctuationClass::Closing
                | PunctuationClass::Middle
                | PunctuationClass::Space
                | PunctuationClass::Ps
                | PunctuationClass::Pe
        ) || CodePointMapData::<GeneralCategory>::new().get(ch)
            == GeneralCategory::DashPunctuation
            || matches!(
                ch,
                '\u{ad}' | '\'' | '"' | '！' | '？' | '‼' | '⁇' | '⁈' | '⁉'
            );
        (ch, japanese, protected)
    };
    let (a, a_japanese, a_protected) = profile(left);
    let (b, b_japanese, b_protected) = profile(right);
    if CodePointMapData::<LineBreak>::new().get(a) == LineBreak::Inseparable
        && CodePointMapData::<LineBreak>::new().get(b) == LineBreak::Inseparable
        || matches!(a, '—' | '―') && matches!(b, '—' | '―')
    {
        return false;
    }
    !(a_japanese || b_japanese) || !(a_protected || b_protected)
}

#[cfg(test)]
mod tests;
