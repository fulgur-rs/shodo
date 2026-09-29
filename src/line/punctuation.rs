//! Punctuation classification and blank space from the actual shaping face.
use crate::geometry::{LayoutUnit, Saturation};
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
        let nominal = font
            .charmap()
            .map('水')
            .and_then(|g| metrics.advance_width(g))
            .unwrap_or(run.font_size);
        // A proportional ideograph face cannot supply a dependable fullwidth
        // measure. Only compare characters that the retained face contains.
        let proportional = ['卜', '一']
            .iter()
            .filter_map(|c| {
                font.charmap()
                    .map(*c)
                    .and_then(|g| metrics.advance_width(g))
            })
            .any(|w| (w - nominal).abs() > 1.0 / 64.0);
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
                value.start_trim = if ltr { p.left } else { p.right };
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
                value.end_trim = (if ltr { p.right } else { p.left }).min(remaining);
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
mod tests {
    use super::{PunctuationClass as P, classify};
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::node::{NodeId, TextSource};
    use crate::style::{
        FontFamily, FontMetricKind, FontSizeAdjust, FontVariation, InlineStyle, ParagraphStyle,
        TextSpacingTrim,
    };
    use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};
    use skrifa::{FontRef, MetadataProvider, raw::TableProvider};

    fn cjk_tables() -> Vec<([u8; 4], Vec<u8>)> {
        let base = include_bytes!("../../dev/fixtures/assets/fonts/cjk.otf");
        (0..u16::from_be_bytes(base[4..6].try_into().unwrap()) as usize)
            .map(|n| {
                let at = 12 + n * 16;
                let offset = u32::from_be_bytes(base[at + 8..at + 12].try_into().unwrap()) as usize;
                let len = u32::from_be_bytes(base[at + 12..at + 16].try_into().unwrap()) as usize;
                (
                    base[at..at + 4].try_into().unwrap(),
                    base[offset..offset + len].to_vec(),
                )
            })
            .collect()
    }

    fn cjk_font(tables: &mut [([u8; 4], Vec<u8>)]) -> Vec<u8> {
        tables.sort_by_key(|t| t.0);
        let mut bytes = crate::font::sfnt::build_sfnt(tables);
        bytes[..4].copy_from_slice(b"OTTO");
        bytes
    }

    #[test]
    fn justification_filters_boundaries_inside_an_actual_punctuation_ligature() {
        use crate::Fragment;
        use crate::style::{TextAlign, TextJustify};
        let font = FontRef::new(include_bytes!("../../dev/fixtures/assets/fonts/cjk.otf")).unwrap();
        let ids: Vec<_> = ['「', '日', '」']
            .iter()
            .map(|c| font.charmap().map(*c).unwrap().to_u32() as u16)
            .collect();
        let mut gsub = Vec::new();
        let words = |bytes: &mut Vec<u8>, values: &[u16]| {
            for value in values {
                bytes.extend(value.to_be_bytes());
            }
        };
        // GSUB1.0: DFLT required rlig, one type4/format1 lookup mapping
        // 「日」 to the retained 日 outline. Each input keeps its source cut.
        words(&mut gsub, &[1, 0, 10, 30, 44, 1]);
        gsub.extend(b"DFLT");
        words(&mut gsub, &[8, 4, 0, 0, 0, 0, 0, 1]);
        gsub.extend(b"rlig");
        words(&mut gsub, &[8, 0, 1, 0, 1, 4, 4, 0, 1, 8]);
        words(
            &mut gsub,
            &[1, 8, 1, 14, 1, 1, ids[0], 1, 4, ids[1], 3, ids[1], ids[2]],
        );
        let mut tables = cjk_tables();
        tables.retain(|(tag, _)| tag != b"GSUB");
        tables.push((*b"GSUB", gsub));
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                cjk_font(&mut tables),
                0,
                FontFaceDescriptor {
                    family: "Ligature".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = ParagraphStyle {
            root: InlineStyle {
                font_families: vec![FontFamily::Named("Ligature".into())],
                font_size: 16.,
                lang: Some("ja".into()),
                text_spacing_trim: TextSpacingTrim::SpaceAll,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "「日」");
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        assert_eq!(
            p.data.glyphs.id,
            [ids[1] as u32],
            "fixture must actually ligate"
        );
        let options = crate::style::LineOptions {
            text_align: TextAlign::JustifyAll,
            text_justify: TextJustify::InterCharacter,
            ..Default::default()
        };
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &options,
            &LineConstraint::new(48.),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("line")
        };
        assert_eq!(line.text_range(), 0..9);
        assert_eq!(line.inline_size(), 16.);
        let run = line
            .fragments()
            .find_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        let glyphs: Vec<_> = run.glyphs().collect();
        assert_eq!(glyphs.len(), 1);
        assert_eq!(glyphs[0].inline_position, 16.);
    }

    #[test]
    fn fallback_variation_and_size_adjust_use_the_actual_blank() {
        let mut tables = cjk_tables();
        let count = FontRef::new(include_bytes!("../../dev/fixtures/assets/fonts/cjk.otf"))
            .unwrap()
            .maxp()
            .unwrap()
            .num_glyphs();
        let mut fvar = Vec::new();
        for value in [1u16, 0, 16, 2, 1, 20, 0, 8] {
            fvar.extend(value.to_be_bytes());
        }
        fvar.extend(b"wght");
        for value in [100i32, 400, 900] {
            fvar.extend((value << 16).to_be_bytes());
        }
        fvar.extend([0, 0, 1, 0]);
        // A real HVAR table adds 600 font units to every advance at wght=900.
        // The outlines retain their original bounds, so halfwidth trimming
        // becomes unsafe after the ic-width adjustment keeps advances at 32px.
        let mut hvar = Vec::new();
        for value in [1u16, 0] {
            hvar.extend(value.to_be_bytes());
        }
        for value in [20u32, 0, 0, 0] {
            hvar.extend(value.to_be_bytes());
        }
        hvar.extend(1u16.to_be_bytes());
        hvar.extend(12u32.to_be_bytes());
        hvar.extend(1u16.to_be_bytes());
        hvar.extend(22u32.to_be_bytes());
        for value in [1u16, 1, 0, 16384, 16384, count, 1, 1, 0] {
            hvar.extend(value.to_be_bytes());
        }
        for _ in 0..count {
            hvar.extend(600i16.to_be_bytes());
        }
        tables.push((*b"fvar", fvar));
        tables.push((*b"HVAR", hvar));
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        fonts
            .register_face(
                cjk_font(&mut tables),
                0,
                FontFaceDescriptor {
                    family: "Variable".into(),
                    weight: (100., 900.),
                    ..Default::default()
                },
            )
            .unwrap();
        for (weight, expected, blank) in [(400., 80., 16.), (900., 96., 0.)] {
            let style = ParagraphStyle {
                root: InlineStyle {
                    font_families: vec![
                        FontFamily::Named("Latin".into()),
                        FontFamily::Named("Variable".into()),
                    ],
                    font_size: 16.,
                    lang: Some("ja".into()),
                    font_variations: vec![FontVariation {
                        tag: *b"wght",
                        value: weight,
                    }],
                    font_size_adjust: Some(FontSizeAdjust {
                        metric: FontMetricKind::IcWidth,
                        value: 2.,
                    }),
                    text_spacing_trim: TextSpacingTrim::Normal,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut builder = ParagraphBuilder::new(&style, &limits);
            builder.push_text(TextSource::Generated { node: NodeId(1) }, "「「日");
            let para = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
            assert_eq!(
                para.data.punctuation[0].advance.to_f32(),
                32.,
                "weight {weight}"
            );
            assert_eq!(
                para.data.punctuation[0].left.to_f32(),
                blank,
                "weight {weight}"
            );
            // The default location is represented by empty normalized coords.
            if weight == 900. {
                assert!(para.data.runs.iter().all(|r| r.instance.coords.len() == 1));
            }
            let LineResult::Line(line) = para.next_line(
                &mut LayoutContext::new(),
                para.start_token(),
                &Default::default(),
                &LineConstraint::new(200.),
                &AtomicSizes::EMPTY,
            ) else {
                panic!("line")
            };
            assert_eq!(line.inline_size(), expected, "weight {weight}");
            assert_eq!(line.text_range(), 0..9);
        }
    }

    #[test]
    fn proportional_punctuation_keeps_its_advance_and_ink() {
        let font = FontRef::new(include_bytes!("../../dev/fixtures/assets/fonts/cjk.otf")).unwrap();
        let opening = font.charmap().map('「').unwrap().to_u32() as usize;
        let mut tables = cjk_tables();
        let hmtx = &mut tables.iter_mut().find(|t| t.0 == *b"hmtx").unwrap().1;
        hmtx[opening * 4..opening * 4 + 2].copy_from_slice(&800u16.to_be_bytes());
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                cjk_font(&mut tables),
                0,
                FontFaceDescriptor {
                    family: "Proportional".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = ParagraphStyle {
            root: InlineStyle {
                font_families: vec![FontFamily::Named("Proportional".into())],
                font_size: 20.,
                lang: Some("ja".into()),
                text_spacing_trim: TextSpacingTrim::TrimAll,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "「「日");
        let para = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        assert_eq!(para.data.punctuation[0].advance.to_f32(), 16.);
        assert_eq!(para.data.punctuation[0].left.to_f32(), 0.);
        let LineResult::Line(line) = para.next_line(
            &mut LayoutContext::new(),
            para.start_token(),
            &Default::default(),
            &LineConstraint::new(200.),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("line")
        };
        assert_eq!(line.inline_size(), 52.);
    }

    #[test]
    fn chinese_punctuation_respects_script_and_region_subtags() {
        for lang in [
            "zh-Hant",
            "zh-Hant-HK",
            "ZH-hant-tw",
            "zh-TW",
            "zh-HK",
            "zh-MO",
            "zh-TW-x-test",
        ] {
            for ch in ['、', '。', '，', '．', '：', '；'] {
                assert_eq!(classify(ch, Some(lang)), P::Middle, "{lang}: {ch}");
            }
        }
        for lang in [
            "zh",
            "zh-CN",
            "zh-Hans-CN",
            "zh-Hans-TW",
            "zh-SG",
            "zh-Hans-x-test",
        ] {
            for ch in ['、', '。', '，', '．', '：', '；'] {
                assert_eq!(classify(ch, Some(lang)), P::Closing, "{lang}: {ch}");
            }
        }
        for lang in [
            None,
            Some("ja-JP"),
            Some("en-Hant"),
            Some("zhx"),
            Some("zh-u-rg-twzzzz"),
        ] {
            assert_eq!(classify('、', lang), P::Closing, "{lang:?}");
        }
        for lang in [None, Some("ja-JP"), Some("en-Hant"), Some("zhx")] {
            assert_eq!(classify('：', lang), P::Middle, "{lang:?}");
        }
    }
}
