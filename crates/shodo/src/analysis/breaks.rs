//! Whole-IFC segmentation with CSS tailoring and explicit emergency breaks.
use crate::analysis::units::BreakClass;
use crate::analysis::whitespace::Processed;
use crate::analysis::{Item, ItemKind};
use crate::limits::{WarningKind, WarningSink};
use crate::style::{
    Hyphens, InlineStyle, LineBreak, OverflowWrap, TextWrapMode, WhiteSpaceCollapse, WordBreak,
};
use icu_locale_core::LanguageIdentifier;
use icu_properties::{CodePointMapData, CodePointSetData, props};
use icu_segmenter::options::{LineBreakOptions, LineBreakStrictness, LineBreakWordOption};
use icu_segmenter::{GraphemeClusterSegmenter, LineSegmenter};
use std::collections::BTreeSet;
use std::ops::Range;

/// A soft line-break opportunity after CSS and Unicode line-break analysis.
/// Mandatory breaks are not passed to a caller override.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoftBreakOpportunity {
    /// No ordinary soft break is available.
    Prohibited,
    /// An ordinary soft break is available.
    Allowed,
    /// A break is available only when an unbreakable run overflows.
    Emergency,
    /// A break is available with a visible hyphen.
    Hyphen,
}

/// Input to a caller's line-break override during paragraph construction.
/// `offset` is a UTF-8 byte boundary in `text`, after whitespace processing
/// and text transformation. The text can contain inserted bidi controls.
#[derive(Clone, Copy, Debug)]
pub struct LineBreakContext<'a> {
    /// Processed paragraph text. Inserted bidi controls may be present.
    pub text: &'a str,
    /// UTF-8 byte boundary in `text` immediately after the preceding content.
    pub offset: usize,
    /// The standard opportunity at this boundary.
    pub standard: SoftBreakOpportunity,
}

/// Caller decision for an eligible soft boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineBreakOverride {
    /// Preserve the standard Unicode/CSS opportunity, including hyphenation.
    UseStandard,
    /// Allow an ordinary soft break here, without an inserted hyphen.
    Allow,
    /// Forbid this soft break, including an emergency or hyphen opportunity.
    Prohibit,
}

pub(crate) type OverrideCallback =
    dyn for<'a> Fn(LineBreakContext<'a>) -> LineBreakOverride + Send + Sync;

#[derive(Clone, Copy, Debug)]
pub(crate) struct BreakOpportunity {
    pub(crate) offset: u32,
    pub(crate) class: BreakClass,
    pub(crate) min_content: bool,
}

pub(crate) struct BreakAnalysis {
    pub(crate) graphemes: Vec<u32>,
    /// Actual character starts after transparent bidi controls. Break cuts
    /// intentionally stay before those controls for source consumption.
    pub(crate) typographic_starts: Vec<u32>,
    /// Authored controls omitted from projection but retained in shaping text.
    pub(crate) authored_bidi_controls: Vec<u32>,
    pub(crate) caret_cuts: Vec<u32>,
    pub(crate) opportunities: Vec<BreakOpportunity>,
}

impl BreakAnalysis {
    pub(crate) fn at(&self, offset: u32) -> BreakOpportunity {
        self.opportunities
            .binary_search_by_key(&offset, |o| o.offset)
            .ok()
            .map_or(
                BreakOpportunity {
                    offset,
                    class: BreakClass::Prohibited,
                    min_content: false,
                },
                |i| self.opportunities[i],
            )
    }

    /// Materialize the callback before building units, so all layout and
    /// intrinsic-width paths observe the same opportunities.
    pub(crate) fn apply_override(
        &mut self,
        input: &Processed,
        styles: &[InlineStyle],
        combine_spans: &[super::combine::CombineSpan],
        callback: &OverrideCallback,
    ) {
        let end = self.graphemes.last().copied().unwrap_or(0);
        for opportunity in &mut self.opportunities {
            let offset = opportunity.offset;
            if offset == 0 || offset == end || opportunity.class == BreakClass::Mandatory {
                continue;
            }
            let Some(item) = item_before(&input.items, offset) else {
                continue;
            };
            if styles[item.style as usize].text_wrap_mode == TextWrapMode::NoWrap {
                continue;
            }
            let indivisible = input
                .indivisible
                .partition_point(|range| range.end <= offset);
            if input
                .indivisible
                .get(indivisible)
                .is_some_and(|range| range.start < offset)
            {
                continue;
            }
            let combined = combine_spans.partition_point(|span| span.text.end <= offset);
            if combine_spans
                .get(combined)
                .is_some_and(|span| span.text.start < offset)
            {
                continue;
            }
            let standard = match opportunity.class {
                BreakClass::Prohibited => SoftBreakOpportunity::Prohibited,
                BreakClass::Allowed => SoftBreakOpportunity::Allowed,
                BreakClass::Emergency => SoftBreakOpportunity::Emergency,
                BreakClass::Hyphen => SoftBreakOpportunity::Hyphen,
                BreakClass::Mandatory => unreachable!(),
            };
            match callback(LineBreakContext {
                text: &input.text,
                offset: offset as usize,
                standard,
            }) {
                LineBreakOverride::UseStandard => {}
                LineBreakOverride::Allow => {
                    opportunity.class = BreakClass::Allowed;
                    opportunity.min_content = true;
                }
                LineBreakOverride::Prohibit => {
                    opportunity.class = BreakClass::Prohibited;
                    opportunity.min_content = false;
                }
            }
        }
    }
}

struct ProjectionSpan {
    original: Range<u32>,
    projected: Range<u32>,
}
struct Projection {
    text: String,
    spans: Vec<ProjectionSpan>,
    authored_bidi_controls: Vec<u32>,
}
impl Projection {
    fn new(input: &Processed) -> Self {
        let mut p = Self {
            text: String::new(),
            spans: Vec::new(),
            authored_bidi_controls: Vec::new(),
        };
        let bidi_control = CodePointSetData::new::<props::BidiControl>();
        for item in &input.items {
            if matches!(
                item.kind,
                ItemKind::OutOfFlow { .. } | ItemKind::BidiControl
            ) {
                continue;
            }
            for (i, c) in
                input.text[item.text.start as usize..item.text.end as usize].char_indices()
            {
                if bidi_control.contains(c) {
                    if matches!(item.kind, ItemKind::Text) {
                        p.authored_bidi_controls.push(item.text.start + i as u32);
                    }
                    continue;
                }
                let original = item.text.start + i as u32;
                let begin = p.text.len() as u32;
                p.text.push(c);
                let next = ProjectionSpan {
                    original: original..original + c.len_utf8() as u32,
                    projected: begin..p.text.len() as u32,
                };
                if let Some(last) = p.spans.last_mut()
                    && last.original.end == next.original.start
                    && last.projected.end == next.projected.start
                {
                    last.original.end = next.original.end;
                    last.projected.end = next.projected.end;
                } else {
                    p.spans.push(next);
                }
            }
        }
        p
    }
    // A break before a transparent marker belongs to the preceding content.
    fn upstream(&self, offset: usize) -> u32 {
        let index = self
            .spans
            .partition_point(|s| s.projected.end < offset as u32);
        self.spans
            .get(index)
            .map_or(0, |s| s.original.start + offset as u32 - s.projected.start)
    }
}

/// Common input grapheme cuts include the transparent-marker gap at a cut.
/// A transform can consume a combining scalar beyond such a marker; only
/// these source cuts may switch from the alternate to the normal text set.
pub(crate) fn source_cursor_ranges(input: &Processed) -> Vec<std::ops::RangeInclusive<u32>> {
    let projection = Projection::new(input);
    GraphemeClusterSegmenter::new()
        .segment_str(&projection.text)
        .map(|offset| {
            let before = if offset == 0 {
                0
            } else {
                projection.upstream(offset)
            };
            let index = projection
                .spans
                .partition_point(|s| s.projected.end <= offset as u32);
            let after = projection
                .spans
                .get(index)
                .map_or(input.text.len() as u32, |s| {
                    s.original.start + offset as u32 - s.projected.start
                });
            before..=after
        })
        .collect()
}

// At most 4*4*2=32 active profiles can each run a segmenter pass,
// independently of the number of distinct inline styles. Manual uses ICU's
// normal word option but needs its own post-filtering profile.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Profile {
    strict: u8,
    word: u8,
    ja_zh: bool,
}
impl Profile {
    fn new(s: &InlineStyle) -> Self {
        let lang = s
            .lang
            .as_deref()
            .unwrap_or("")
            .split('-')
            .next()
            .unwrap_or("");
        Self {
            strict: match s.line_break {
                LineBreak::Auto | LineBreak::Normal => 0,
                LineBreak::Loose => 1,
                LineBreak::Strict => 2,
                LineBreak::Anywhere => 3,
            },
            word: match s.word_break {
                WordBreak::Normal | WordBreak::AutoPhrase => 0,
                WordBreak::BreakAll => 1,
                WordBreak::KeepAll => 2,
                WordBreak::Manual => 3,
            },
            ja_zh: lang.eq_ignore_ascii_case("ja") || lang.eq_ignore_ascii_case("zh"),
        }
    }
}

fn item_before(items: &[Item], offset: u32) -> Option<&Item> {
    let first = items.partition_point(|item| item.text.end < offset);
    items[first..]
        .iter()
        .take_while(|item| item.text.start < offset)
        .find(|item| {
            !item.text.is_empty()
                && !matches!(
                    item.kind,
                    ItemKind::OutOfFlow { .. } | ItemKind::BidiControl
                )
        })
}

pub(crate) fn analyze_breaks(
    input: &Processed,
    styles: &[InlineStyle],
    warnings: &mut WarningSink,
) -> BreakAnalysis {
    let projection = Projection::new(input);
    let segmenter = GraphemeClusterSegmenter::new();
    let boundaries = segmenter.segment_str(&projection.text);
    #[cfg(test)]
    let boundaries = tests::observe_graphemes(boundaries);
    let (graphemes, typographic_starts): (Vec<u32>, Vec<u32>) = boundaries
        .map(|at| {
            let index = projection
                .spans
                .partition_point(|s| s.projected.end <= at as u32);
            let start = projection
                .spans
                .get(index)
                .map_or(input.text.len() as u32, |s| {
                    s.original.start + at as u32 - s.projected.start
                });
            (projection.upstream(at), start)
        })
        .unzip();
    let mut caret_cuts: Vec<_> = graphemes
        .iter()
        .chain(&typographic_starts)
        .copied()
        .collect();
    caret_cuts.sort_unstable();
    caret_cuts.dedup();
    caret_cuts.retain(|cut| {
        let index = input.indivisible.partition_point(|r| r.end <= *cut);
        !input
            .indivisible
            .get(index)
            .is_some_and(|r| r.start < *cut && *cut < r.end)
    });
    let mut opportunities: Vec<_> = graphemes
        .iter()
        .map(|offset| BreakOpportunity {
            offset: *offset,
            class: BreakClass::Prohibited,
            min_content: false,
        })
        .collect();
    let profiles: Vec<_> = styles.iter().map(Profile::new).collect();
    let active: BTreeSet<_> = input
        .items
        .iter()
        .filter(|i| {
            !i.text.is_empty()
                && !matches!(i.kind, ItemKind::OutOfFlow { .. } | ItemKind::BidiControl)
        })
        .map(|i| profiles[i.style as usize])
        .collect();
    for s in styles {
        if s.word_break == WordBreak::AutoPhrase {
            warnings.push(
                WarningKind::Unsupported,
                "phrase segmentation unavailable; using normal word breaks",
            );
        }
        if s.hyphens == Hyphens::Auto {
            warnings.push(
                WarningKind::Unsupported,
                "automatic hyphenation unavailable; using manual soft hyphens",
            );
        }
    }
    let ja: LanguageIdentifier = "ja".parse().expect("constant language");
    let lb = CodePointMapData::<props::LineBreak>::new();
    // CSS Text 4 word-break:manual resolves SA letters as AL. Substitute AL
    // scalars of the same UTF-8 width so ICU applies all UAX #14 rules while
    // its returned byte offsets still index the original projection.
    let manual_text = active
        .iter()
        .any(|profile| profile.word == 3 && profile.strict != 3)
        .then(|| {
            let gc = CodePointMapData::<props::GeneralCategory>::new();
            projection
                .text
                .chars()
                .map(|c| {
                    if lb.get(c) == props::LineBreak::ComplexContext
                        && !matches!(
                            gc.get(c),
                            props::GeneralCategory::NonspacingMark
                                | props::GeneralCategory::SpacingMark
                        )
                    {
                        match c.len_utf8() {
                            1 => 'a',
                            2 => 'Ā',
                            3 => 'ꓐ',
                            _ => '𐐀',
                        }
                    } else {
                        c
                    }
                })
                .collect::<String>()
        });
    for profile in active {
        let mut options = LineBreakOptions::default();
        options.strictness = Some(match profile.strict {
            0 => LineBreakStrictness::Normal,
            1 => LineBreakStrictness::Loose,
            2 => LineBreakStrictness::Strict,
            _ => LineBreakStrictness::Anywhere,
        });
        options.word_option = Some(match profile.word {
            1 => LineBreakWordOption::BreakAll,
            2 => LineBreakWordOption::KeepAll,
            _ => LineBreakWordOption::Normal,
        });
        options.content_locale = profile.ja_zh.then_some(&ja);
        #[cfg(feature = "complex-scripts")]
        let line = LineSegmenter::new_auto(options);
        #[cfg(not(feature = "complex-scripts"))]
        let line = LineSegmenter::new_for_non_complex_scripts(options);
        let segment_text = if profile.word == 3 && profile.strict != 3 {
            manual_text.as_deref().unwrap_or(&projection.text)
        } else {
            &projection.text
        };
        for at in line.segment_str(segment_text) {
            // ICU's Normal mode relaxes CJ as well as CJK hyphen-like
            // characters. CSS Text §6.2 only permits CJ (small kana and
            // prolonged sound marks) in Loose mode. Tailor the projected
            // boundary so transparent source items cannot hide the CJ.
            if profile.strict == 0
                && projection.text[at..]
                    .chars()
                    .next()
                    .is_some_and(|c| lb.get(c) == props::LineBreak::ConditionalJapaneseStarter)
            {
                continue;
            }
            let offset = projection.upstream(at);
            if let Some(item) = item_before(&input.items, offset)
                && profiles[item.style as usize] == profile
                && let Ok(index) = graphemes.binary_search(&offset)
            {
                opportunities[index].class = BreakClass::Allowed;
                opportunities[index].min_content = true;
            }
        }
    }
    let logical_end = projection.upstream(projection.text.len());
    let mut indivisible_index = 0;
    let mut following_span = 0;
    let mut following_item = 0;
    for o in &mut opportunities {
        if o.offset == 0 {
            o.class = BreakClass::Prohibited;
            o.min_content = false;
            continue;
        }
        let Some(item) = item_before(&input.items, o.offset) else {
            continue;
        };
        let s = &styles[item.style as usize];
        let last = input.text[item.text.start as usize..o.offset as usize]
            .chars()
            .next_back();
        if matches!(item.kind, ItemKind::ForcedBreak)
            || last.is_some_and(|c| {
                matches!(
                    lb.get(c),
                    props::LineBreak::MandatoryBreak | props::LineBreak::NextLine
                )
            })
        {
            o.class = BreakClass::Mandatory;
            o.min_content = true;
        } else if o.offset == logical_end || s.text_wrap_mode == TextWrapMode::NoWrap {
            o.class = BreakClass::Prohibited;
            o.min_content = false;
        } else if s.line_break == LineBreak::Anywhere {
            // ICU's opportunities include preserved whitespace; do not
            // replace them with the normal nonbreaking-space sequence rule.
        } else if matches!(last, Some(' ' | '\t'))
            && matches!(
                s.white_space_collapse,
                WhiteSpaceCollapse::Preserve
                    | WhiteSpaceCollapse::PreserveSpaces
                    | WhiteSpaceCollapse::BreakSpaces
            )
        {
            let index = projection
                .spans
                .partition_point(|span| span.original.end <= o.offset);
            let next = projection.spans.get(index).and_then(|span| {
                input.text[span.original.start.max(o.offset) as usize..]
                    .chars()
                    .next()
            });
            let unicode_prohibited = o.class == BreakClass::Prohibited
                && next.is_some_and(|c| {
                    matches!(
                        lb.get(c),
                        props::LineBreak::WordJoiner
                            | props::LineBreak::Glue
                            | props::LineBreak::ZWJ
                    )
                });
            let allowed = !unicode_prohibited
                && (s.white_space_collapse == WhiteSpaceCollapse::BreakSpaces
                    || !matches!(next, Some(' ' | '\t')));
            o.class = if allowed {
                BreakClass::Allowed
            } else {
                BreakClass::Prohibited
            };
            o.min_content = allowed;
        } else if last == Some('\u{AD}') && s.line_break != LineBreak::Anywhere {
            while projection
                .spans
                .get(following_span)
                .is_some_and(|span| span.original.end <= o.offset)
            {
                following_span += 1;
            }
            let mandatory_follows = projection.spans.get(following_span).is_some_and(|span| {
                let pos = span.original.start.max(o.offset);
                while input
                    .items
                    .get(following_item)
                    .is_some_and(|item| item.text.end <= pos)
                {
                    following_item += 1;
                }
                input.items.get(following_item).is_some_and(|item| {
                    matches!(item.kind, ItemKind::ForcedBreak | ItemKind::BlockInInline)
                }) || input.text[pos as usize..].chars().next().is_some_and(|c| {
                    matches!(
                        lb.get(c),
                        props::LineBreak::MandatoryBreak | props::LineBreak::NextLine
                    )
                })
            });
            o.class = if s.hyphens == Hyphens::None || mandatory_follows {
                BreakClass::Prohibited
            } else {
                BreakClass::Hyphen
            };
            o.min_content = o.class == BreakClass::Hyphen;
        } else if o.class == BreakClass::Prohibited {
            #[cfg(not(feature = "complex-scripts"))]
            if s.word_break != WordBreak::Manual
                && last.is_some_and(|c| lb.get(c) == props::LineBreak::ComplexContext)
            {
                o.class = BreakClass::Allowed;
                o.min_content = true;
            }
            if o.class == BreakClass::Prohibited && s.overflow_wrap != OverflowWrap::Normal {
                o.class = BreakClass::Emergency;
                o.min_content = s.overflow_wrap == OverflowWrap::Anywhere;
            }
        }
        // Expanded transform scalars remain a single typographic unit even
        // with optional DOM mapping turned off.
        while input
            .indivisible
            .get(indivisible_index)
            .is_some_and(|r| r.end <= o.offset)
        {
            indivisible_index += 1;
        }
        if input
            .indivisible
            .get(indivisible_index)
            .is_some_and(|r| r.start < o.offset && o.offset < r.end)
        {
            o.class = BreakClass::Prohibited;
            o.min_content = false;
        }
    }
    BreakAnalysis {
        graphemes,
        typographic_starts,
        authored_bidi_controls: projection.authored_bidi_controls,
        caret_cuts,
        opportunities,
    }
}
#[cfg(test)]
mod tests {
    use super::{BreakAnalysis, analyze_breaks};
    use crate::analysis::units::BreakClass;
    use crate::analysis::{process, transform};
    use crate::builder::ParagraphBuilder;
    use crate::limits::{Limits, WarningSink};
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{
        Hyphens, InlineStyle, LineBreak, OverflowWrap, ParagraphStyle, TextTransform, TextWrapMode,
        WhiteSpaceCollapse, WordBreak,
    };

    std::thread_local! {
        static GRAPHEME_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub(super) fn observe_graphemes(
        boundaries: impl Iterator<Item = usize>,
    ) -> impl Iterator<Item = usize> {
        boundaries.inspect(|_| GRAPHEME_VISITS.with(|visits| visits.set(visits.get() + 1)))
    }

    fn analyze(text: &str, style: InlineStyle, mapping: bool) -> BreakAnalysis {
        let limits = Limits::default();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style,
                ..ParagraphStyle::default()
            },
            &limits,
        );
        b.with_offset_mapping(mapping).push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            text,
        );
        let processed = process(&b.text, &b.items, &b.styles, mapping, &limits).unwrap();
        let mut warnings = WarningSink::default();
        let processed = transform(
            processed,
            &b.styles,
            &limits,
            &mut warnings,
            b.style.writing_mode,
        )
        .unwrap();
        analyze_breaks(&processed, &b.styles, &mut warnings)
    }

    #[test]
    fn grapheme_boundaries_are_segmented_once_for_both_coordinate_tables() {
        let cases: &[(&str, &[u32])] = &[
            ("", &[0]),
            ("ab", &[0, 1, 2]),
            ("e\u{301}👩\u{200D}💻 x", &[0, 3, 14, 15, 16]),
            ("🇯🇵x", &[0, 8, 9]),
        ];
        for &(text, expected) in cases {
            GRAPHEME_VISITS.with(|visits| visits.set(0));
            let result = analyze(text, InlineStyle::default(), true);
            assert_eq!(result.graphemes.as_slice(), expected, "{text:?}");
            assert_eq!(result.typographic_starts.as_slice(), expected, "{text:?}");
            assert_eq!(
                GRAPHEME_VISITS.with(std::cell::Cell::get),
                expected.len(),
                "each actual ICU boundary should be visited once for {text:?}"
            );
        }
    }

    #[test]
    fn projected_graphemes_keep_both_sides_of_transparent_source_gaps() {
        let bidi = analyze("\u{200E}a\u{200F}b\u{200E}", InlineStyle::default(), true);
        assert_eq!(bidi.graphemes, vec![3, 4, 8]);
        assert_eq!(bidi.typographic_starts, vec![3, 7, 11]);
        assert_eq!(bidi.caret_cuts, vec![3, 4, 7, 8, 11]);

        let limits = Limits::default();
        let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        builder
            .push_text(TextSource::Generated { node: NodeId(1) }, "a")
            .push_out_of_flow(NodeId(2), crate::node::OutOfFlowKind::Float)
            .push_text(TextSource::Generated { node: NodeId(3) }, "b");
        let processed = process(
            &builder.text,
            &builder.items,
            &builder.styles,
            true,
            &limits,
        )
        .unwrap();
        assert_eq!(processed.text, "a\u{FFFC}b");
        let result = analyze_breaks(&processed, &builder.styles, &mut WarningSink::default());
        assert_eq!(result.graphemes, vec![0, 1, 5]);
        assert_eq!(result.typographic_starts, vec![0, 4, 5]);
        assert_eq!(result.caret_cuts, vec![0, 1, 4, 5]);
    }

    #[test]
    fn css_japanese_strictness_matrix() {
        // CSS Text 4 §6.2: these are literal wrapping expectations, not
        // expectations computed using ICU's strictness options.
        let rows = [
            ("日〜本", [true, true, false]),
            ("日゠本", [true, true, false]),
            ("日ぁ本", [true, false, false]),
            ("日ー本", [true, false, false]),
            ("日々本", [true, false, false]),
            ("……日", [true, false, false]),
            ("日・本", [true, false, false]),
            ("日：本", [true, false, false]),
            ("日；本", [true, false, false]),
            ("日！本", [true, false, false]),
            ("日？本", [true, false, false]),
            ("日％本", [true, false, false]),
            ("＄日本", [true, false, false]),
            ("日‐本", [true, false, false]),
            ("日–本", [true, false, false]),
        ];
        let mut failures = Vec::new();
        for lang in ["ja", "zh"] {
            for (text, expected) in rows {
                for (mode, allowed) in [LineBreak::Loose, LineBreak::Normal, LineBreak::Strict]
                    .into_iter()
                    .zip(expected)
                {
                    let result = analyze(
                        text,
                        InlineStyle {
                            lang: Some(lang.into()),
                            line_break: mode,
                            ..InlineStyle::default()
                        },
                        true,
                    );
                    let opportunity = result.at(3);
                    let expected = if allowed {
                        BreakClass::Allowed
                    } else {
                        BreakClass::Prohibited
                    };
                    if opportunity.class != expected || opportunity.min_content != allowed {
                        failures.push(format!(
                            "{lang} {mode:?} {text:?}: {:?}/min={} expected {expected:?}/min={allowed}",
                            opportunity.class, opportunity.min_content
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn css_strictness_preserves_other_break_contracts() {
        for lang in [None, Some("en"), Some("ja-JP"), Some("zh-Hant")] {
            for mode in [LineBreak::Loose, LineBreak::Normal, LineBreak::Strict] {
                let style = InlineStyle {
                    lang: lang.map(str::to_owned),
                    line_break: mode,
                    ..InlineStyle::default()
                };
                assert_eq!(
                    analyze("日ぁ本", style.clone(), true).at(3).class,
                    if mode == LineBreak::Loose {
                        BreakClass::Allowed
                    } else {
                        BreakClass::Prohibited
                    },
                    "CJ {lang:?} {mode:?}"
                );
                assert_eq!(
                    analyze("日〜本", style.clone(), true).at(3).class,
                    if matches!(lang, Some("ja-JP" | "zh-Hant")) && mode != LineBreak::Strict {
                        BreakClass::Allowed
                    } else {
                        BreakClass::Prohibited
                    },
                    "hyphen-like {lang:?} {mode:?}"
                );
                assert_eq!(
                    analyze("a‐b", style.clone(), true).at(1).class,
                    BreakClass::Prohibited
                );
                assert_eq!(
                    analyze(
                        "日ぁ本",
                        InlineStyle {
                            text_wrap_mode: TextWrapMode::NoWrap,
                            ..style.clone()
                        },
                        true,
                    )
                    .at(3)
                    .class,
                    BreakClass::Prohibited
                );
                assert_eq!(
                    analyze(
                        "日ぁ本",
                        InlineStyle {
                            line_break: LineBreak::Anywhere,
                            ..style.clone()
                        },
                        true,
                    )
                    .at(3)
                    .class,
                    BreakClass::Allowed
                );
                assert_eq!(
                    analyze(
                        "日本",
                        InlineStyle {
                            word_break: WordBreak::KeepAll,
                            ..style
                        },
                        true,
                    )
                    .at(3)
                    .class,
                    BreakClass::Prohibited
                );
            }
        }
    }

    #[test]
    fn normal_cj_tailoring_looks_through_inline_bidi_controls() {
        for mode in [LineBreak::Normal, LineBreak::Loose, LineBreak::Anywhere] {
            let limits = Limits::default();
            let style = InlineStyle {
                lang: Some("ja".into()),
                line_break: mode,
                ..InlineStyle::default()
            };
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: style.clone(),
                    ..ParagraphStyle::default()
                },
                &limits,
            );
            b.push_text(TextSource::Generated { node: NodeId(1) }, "日")
                .open_inline(NodeId(2), &style, InlineEdges::default())
                .push_text(
                    TextSource::Generated { node: NodeId(3) },
                    "\u{2066}ぁ\u{3099}\u{2069}",
                )
                .close_inline();
            let processed = process(&b.text, &b.items, &b.styles, true, &limits).unwrap();
            let mut warnings = WarningSink::default();
            let processed = transform(
                processed,
                &b.styles,
                &limits,
                &mut warnings,
                b.style.writing_mode,
            )
            .unwrap();
            let breaks = analyze_breaks(&processed, &b.styles, &mut warnings);
            assert_eq!(
                breaks.at(3).class,
                if mode == LineBreak::Normal {
                    BreakClass::Prohibited
                } else {
                    BreakClass::Allowed
                }
            );
            // The combining mark remains in its base character even with
            // anywhere; source byte positions are not new character cuts.
            assert!(breaks.opportunities.iter().all(|o| o.offset != 9));
        }
    }

    #[test]
    fn strictness_and_locale() {
        let ja = InlineStyle {
            lang: Some("ja".to_owned()),
            ..InlineStyle::default()
        };
        assert_eq!(
            analyze(
                "あぁい",
                InlineStyle {
                    line_break: LineBreak::Strict,
                    ..ja.clone()
                },
                true
            )
            .at(3)
            .class,
            BreakClass::Prohibited
        );
        assert_eq!(
            analyze(
                "あぁい",
                InlineStyle {
                    line_break: LineBreak::Loose,
                    ..ja
                },
                true
            )
            .at(3)
            .class,
            BreakClass::Allowed
        );
    }

    #[test]
    fn break_all_keep_all_anywhere() {
        assert_eq!(
            analyze("abc", InlineStyle::default(), true).at(1).class,
            BreakClass::Prohibited
        );
        assert_eq!(
            analyze(
                "abc",
                InlineStyle {
                    word_break: WordBreak::BreakAll,
                    ..InlineStyle::default()
                },
                true
            )
            .at(1)
            .class,
            BreakClass::Allowed
        );
        assert_eq!(
            analyze("日本語", InlineStyle::default(), true).at(3).class,
            BreakClass::Allowed
        );
        assert_eq!(
            analyze(
                "日本語",
                InlineStyle {
                    word_break: WordBreak::KeepAll,
                    ..InlineStyle::default()
                },
                true
            )
            .at(3)
            .class,
            BreakClass::Prohibited
        );
        assert_eq!(
            analyze(
                "a\u{A0}b",
                InlineStyle {
                    line_break: LineBreak::Anywhere,
                    word_break: WordBreak::KeepAll,
                    ..InlineStyle::default()
                },
                true
            )
            .at(1)
            .class,
            BreakClass::Allowed
        );
    }

    #[test]
    fn manual_suppresses_thai_lexical_breaks_but_preserves_explicit_and_anywhere_breaks() {
        let thai = "ภาษาไทยภาษาไทย";
        let boundary = "ภาษา".len() as u32;
        assert_eq!(
            analyze(thai, InlineStyle::default(), true)
                .at(boundary)
                .class,
            BreakClass::Allowed,
            "normal uses Thai lexical word boundaries"
        );
        let manual = InlineStyle {
            word_break: WordBreak::Manual,
            ..InlineStyle::default()
        };
        let blocked = analyze(thai, manual.clone(), true).at(boundary);
        assert_eq!(blocked.class, BreakClass::Prohibited);
        assert!(!blocked.min_content);
        assert_eq!(
            analyze("ภาษา\u{200B}ไทย", manual.clone(), true)
                .at(("ภาษา\u{200B}".len()) as u32)
                .class,
            BreakClass::Allowed,
            "authored zero-width space remains a break opportunity"
        );
        assert_eq!(
            analyze(
                thai,
                InlineStyle {
                    line_break: LineBreak::Anywhere,
                    ..manual.clone()
                },
                true
            )
            .at(boundary)
            .class,
            BreakClass::Allowed
        );
        let emergency = analyze(
            thai,
            InlineStyle {
                overflow_wrap: OverflowWrap::Anywhere,
                ..manual.clone()
            },
            true,
        )
        .at(boundary);
        assert_eq!(emergency.class, BreakClass::Emergency);
        assert!(emergency.min_content);
        assert_eq!(
            analyze("日本語", manual, true).at(3).class,
            BreakClass::Allowed,
            "manual must retain normal CJK breaks"
        );
    }

    #[test]
    fn manual_treats_thai_as_alphabetic_at_other_class_boundaries() {
        let lb = icu_properties::CodePointMapData::<icu_properties::props::LineBreak>::new();
        for c in ['a', 'Ā', 'ꓐ', '𐐀'] {
            assert_eq!(
                lb.get(c),
                icu_properties::props::LineBreak::Alphabetic,
                "{c:?}"
            );
        }
        let manual = InlineStyle {
            word_break: WordBreak::Manual,
            ..InlineStyle::default()
        };
        for text in ["ภาษา๐", "ภาษา§"] {
            assert_eq!(
                analyze(text, InlineStyle::default(), true)
                    .at("ภาษา".len() as u32)
                    .class,
                BreakClass::Allowed,
                "ICU normal initially permits this SA boundary in {text:?}"
            );
        }
        for (text, at) in [
            ("ภาษา๐", "ภาษา".len() as u32),
            ("ภาษา§", "ภาษา".len() as u32),
            ("ภาษา$", "ภาษา".len() as u32),
            ("$ภาษา", "$".len() as u32),
        ] {
            let result = analyze(text, manual.clone(), true).at(at);
            assert_eq!(result.class, BreakClass::Prohibited, "{text:?}");
            assert!(!result.min_content, "{text:?}");
        }
    }

    #[test]
    fn nowrap_no_soft_breaks() {
        let p = analyze(
            "ab\ncd",
            InlineStyle {
                word_break: WordBreak::BreakAll,
                overflow_wrap: OverflowWrap::Anywhere,
                text_wrap_mode: TextWrapMode::NoWrap,
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..InlineStyle::default()
            },
            true,
        );
        assert_eq!(p.at(1).class, BreakClass::Prohibited);
        assert_eq!(p.at(3).class, BreakClass::Mandatory);
        let p = analyze("a\u{2028}b", InlineStyle::default(), true);
        assert_eq!(p.at(4).class, BreakClass::Mandatory);
    }

    #[test]
    fn overflow_wrap_intrinsic_distinction() {
        let p = analyze(
            "abc",
            InlineStyle {
                overflow_wrap: OverflowWrap::BreakWord,
                ..InlineStyle::default()
            },
            true,
        );
        assert_eq!(p.at(1).class, BreakClass::Emergency);
        assert!(!p.at(1).min_content);
        let p = analyze(
            "abc",
            InlineStyle {
                overflow_wrap: OverflowWrap::Anywhere,
                ..InlineStyle::default()
            },
            true,
        );
        assert_eq!(p.at(1).class, BreakClass::Emergency);
        assert!(p.at(1).min_content);
    }

    #[test]
    fn manual_soft_hyphen() {
        assert_eq!(
            analyze("ab\u{AD}cd", InlineStyle::default(), true)
                .at(4)
                .class,
            BreakClass::Hyphen
        );
        assert_eq!(
            analyze(
                "ab\u{AD}cd",
                InlineStyle {
                    hyphens: Hyphens::None,
                    ..InlineStyle::default()
                },
                true
            )
            .at(4)
            .class,
            BreakClass::Prohibited
        );
        assert_eq!(
            analyze(
                "ab\u{AD}cd",
                InlineStyle {
                    line_break: LineBreak::Anywhere,
                    ..InlineStyle::default()
                },
                true
            )
            .at(4)
            .class,
            BreakClass::Allowed
        );
    }

    #[test]
    fn nbsp_word_joiner_zwj_graphemes() {
        let p = analyze(
            "e\u{301}👩\u{200D}💻 x",
            InlineStyle {
                line_break: LineBreak::Anywhere,
                ..InlineStyle::default()
            },
            true,
        );
        assert_eq!(p.graphemes, vec![0, 3, 14, 15, 16]);
        assert_eq!(p.at(3).class, BreakClass::Allowed);
        assert_eq!(p.at(7).class, BreakClass::Prohibited);
        for (text, at) in [("ab\u{A0}cd", 2), ("ab\u{A0}cd", 4), ("ab\u{2060}cd", 2)] {
            assert_eq!(
                analyze(text, InlineStyle::default(), true).at(at).class,
                BreakClass::Prohibited
            );
        }
    }

    #[test]
    fn complex_scripts_feature_modes() {
        let p = analyze("ภาษาไทย", InlineStyle::default(), true);
        #[cfg(feature = "complex-scripts")]
        {
            assert_eq!(p.at(3).class, BreakClass::Prohibited);
            assert_eq!(p.at(12).class, BreakClass::Allowed);
        }
        #[cfg(not(feature = "complex-scripts"))]
        {
            for at in &p.graphemes[1..p.graphemes.len() - 1] {
                assert_eq!(p.at(*at).class, BreakClass::Allowed);
            }
        }
    }

    #[test]
    fn expanded_transform_is_indivisible_without_dom_mapping() {
        for mapping in [true, false] {
            let p = analyze(
                "ßx",
                InlineStyle {
                    text_transform: TextTransform::Uppercase,
                    line_break: LineBreak::Anywhere,
                    ..InlineStyle::default()
                },
                mapping,
            );
            assert_eq!(p.at(1).class, BreakClass::Prohibited);
            assert_eq!(p.at(2).class, BreakClass::Allowed);
        }
    }

    #[test]
    fn many_styles_shared_segmenter_passes() {
        let limits = Limits::default();
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        for i in 0..4096 {
            let s = InlineStyle {
                font_size: 16.0 + i as f32,
                ..InlineStyle::default()
            };
            b.open_inline(NodeId(i + 1), &s, InlineEdges::default())
                .push_text(
                    TextSource::Generated {
                        node: NodeId(i + 10000),
                    },
                    "a",
                )
                .close_inline();
        }
        let p = process(&b.text, &b.items, &b.styles, false, &limits).unwrap();
        GRAPHEME_VISITS.with(|visits| visits.set(0));
        let p = analyze_breaks(&p, &b.styles, &mut WarningSink::default());
        assert_eq!(p.graphemes.len(), 4097);
        assert_eq!(GRAPHEME_VISITS.with(std::cell::Cell::get), 4097);
        assert!(
            p.opportunities
                .iter()
                .all(|o| o.class == BreakClass::Prohibited)
        );
    }
    #[test]
    fn final_forced_separator_stays_mandatory() {
        let s = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..Default::default()
        };
        assert_eq!(analyze("a\n", s, true).at(2).class, BreakClass::Mandatory);
    }

    #[test]
    fn nowrap_atomic_and_tab_units_do_not_introduce_soft_breaks() {
        let limits = Limits::default();
        let root = InlineStyle {
            text_wrap_mode: TextWrapMode::NoWrap,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..Default::default()
        };
        let style = ParagraphStyle {
            root: root.clone(),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a\t")
            .push_atomic(NodeId(2), &root, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "b");
        let fonts = crate::font::FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let mut cx = crate::LayoutContext::new();
        let p = b.build(&mut cx, &fonts).unwrap();
        assert!(
            p.data
                .units
                .iter()
                .all(|u| u.break_after == BreakClass::Prohibited)
        );
    }
}
