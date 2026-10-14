//! Cumulative admission allowances, reserved before analysis producers run.
//!
//! Each phase reserves retained logical payload plus an upper bound on its
//! variable scratch. Dropping scratch does not refund a phase. Repeated
//! allocations inside a dependency (or sequential CSS break profiles) use
//! that phase's scratch allowance: this is neither RSS nor cumulative allocator
//! traffic/CPU accounting. Fixed headers, bounded stacks/model temporaries and
//! allocator/spare capacity are excluded, as for the other logical budgets.
use crate::analysis::{Item, breaks, combine, whitespace::Processed};
use crate::builder::RawItem;
use crate::geometry::WritingMode;
use crate::limits::{LimitExceeded, LimitKind, Limits};
use crate::ruby::base_budget::BaseScopes;
use crate::style::{
    CaseTransform, InlineStyle, ParagraphStyle, TextCombineUpright, TextTransform, WordBreak,
    WordSpaceTransform,
};
use icu_properties::{CodePointMapData, props};
use std::mem::size_of;

crate::logical::logical_sizes! {
    LANGUAGE_IDENTIFIER_BYTES: icu_locale_core::LanguageIdentifier = 32;
}
use std::ops::{Range, RangeInclusive};

// unicode-bidi 0.3.18 (UTF-8): byte classes (original + processing) and levels,
// paragraph flags, level runs, isolating-run sequences and their Vec headers,
// weak indices, bracket pairs (four usize fields) and neutral indices. Each
// variable collection has <= bytes entries; 256 bytes/input byte exceeds the
// sum of their element sizes even if all sequential scratch is included.
// Explicit-status and bracket stacks are bounded by UAX #9 (125 / 63).
const BIDI_SCRATCH_PER_BYTE: u64 = 256;

// ICU 2.3.0 baked auto data: all four SA models use codepoints and 27 hidden
// units. The largest input-sized matrix is 27 f32/scalar (108 bytes); ids,
// complex-run UTF-8 and all simultaneously live boundary/cache arrays fit in
// 512 bytes/scalar. Fixed model weights are borrowed. Without auto, only run
// copies and boundary arrays remain. CJK dictionary iteration has no matrix.
// Re-audit these allowances when updating unicode-bidi or ICU/model data.
#[cfg(feature = "complex-scripts")]
const SA_SCRATCH_PER_SCALAR: u64 = 512;
#[cfg(not(feature = "complex-scripts"))]
const SA_SCRATCH_PER_SCALAR: u64 = 64;
const DICTIONARY_SCRATCH_PER_SCALAR: u64 = 64;

#[derive(Clone, Copy)]
pub(super) struct Budget {
    limit: Option<u64>,
    pub(super) used: u64,
    source_cuts: u64,
}

impl Budget {
    pub(super) fn new(limits: &Limits) -> Self {
        Self {
            limit: limits.max_analysis_bytes,
            used: 0,
            source_cuts: 0,
        }
    }

    fn reserve(&mut self, bytes: u64) -> Result<(), LimitExceeded> {
        let actual = self.used.saturating_add(bytes);
        Limits::check(self.limit, LimitKind::AnalysisBytes, actual)?;
        self.used = actual;
        Ok(())
    }

    pub(super) fn replay_bytes(self) -> u64 {
        // Source cuts belong to the shared pre-transform input, not replay.
        self.used.saturating_sub(self.source_cuts)
    }

    pub(super) fn replay(self, limits: &Limits) -> Result<Self, LimitExceeded> {
        let mut replay = Self::new(limits);
        replay.reserve(self.replay_bytes())?;
        Ok(replay)
    }

    pub(super) fn raw(
        &mut self,
        text: &str,
        items: &[RawItem],
        bases: &mut BaseScopes,
    ) -> Result<(), LimitExceeded> {
        // Byte flags + whitespace barrier offsets (including start/end).
        self.reserve(
            (text.len() as u64).saturating_add(
                (items.len() as u64)
                    .saturating_add(2)
                    .saturating_mul(crate::logical::WORD),
            ),
        )?;
        if bases.enabled() {
            for item in items {
                bases.before_raw(item);
                let bytes = match item {
                    RawItem::Text { range, .. } => u64::from(range.end - range.start),
                    _ => 0,
                };
                bases.analysis_raw(bytes.saturating_add(crate::logical::WORD))?;
                bases.after_raw(item);
            }
        }
        Ok(())
    }

    pub(super) fn source_cuts(
        &mut self,
        input: &Processed,
        bases: &mut BaseScopes,
    ) -> Result<(), LimitExceeded> {
        let before = self.used;
        self.processed(
            input,
            size_of::<RangeInclusive<u32>>() as u64,
            bases,
            |_, text| {
                projection(text).saturating_add(
                    (text.chars().count() as u64)
                        .saturating_mul(size_of::<RangeInclusive<u32>>() as u64),
                )
            },
        )?;
        self.source_cuts = self.used.saturating_sub(before);
        Ok(())
    }

    pub(super) fn transform(
        &mut self,
        input: &Processed,
        styles: &[InlineStyle],
        mode: WritingMode,
        bases: &mut BaseScopes,
    ) -> Result<(), LimitExceeded> {
        let vertical = vertical(mode);
        let transforms = vertical
            || styles.iter().any(|s| {
                s.text_transform != TextTransform::None
                    || s.word_space_transform != WordSpaceTransform::None
            });
        let case = styles
            .iter()
            .any(|s| !matches!(s.text_transform.components().0, CaseTransform::None));
        // Locale parsing may create variants/extensions before extracting the
        // LanguageIdentifier. A byte can introduce at most one tiny token or
        // node, so 64 bytes per input language byte bounds those payloads.
        let locale = |s: &InlineStyle| {
            if transforms {
                LANGUAGE_IDENTIFIER_BYTES.saturating_add(
                    (s.lang.as_ref().map_or(0, String::len) as u64).saturating_mul(64),
                )
            } else {
                0
            }
        };
        self.styles(styles, bases, locale)?;
        self.processed(input, 0, bases, |_, text| {
            let mut bytes = 0u64;
            if vertical {
                bytes = combine::ANALYSIS_ITEM_BYTES.saturating_add(
                    (text.chars().count() as u64).saturating_mul(
                        (size_of::<(Range<u32>, bool, bool)>() + size_of::<Range<u32>>()) as u64,
                    ),
                );
            }
            if case {
                // Logical same-width text copy + byte-indexed u16 case flags,
                // and WordSegmenter scratch before context() is entered.
                bytes = bytes
                    .saturating_add((text.len() as u64).saturating_mul(3))
                    .saturating_add(complex_scratch(text, true));
            }
            bytes
        })
    }

    pub(super) fn analysis(
        &mut self,
        input: &Processed,
        styles: &[InlineStyle],
        style: &ParagraphStyle,
        first_line: bool,
        bases: &mut BaseScopes,
    ) -> Result<(), LimitExceeded> {
        let vertical = vertical(style.writing_mode);
        let internal_bidi = vertical
            && styles
                .iter()
                .any(|s| s.text_combine_upright != TextCombineUpright::None);
        let direction = crate::analysis::bidi::used_root_direction(style, &styles[0]);
        let bidi = crate::analysis::bidi::needs_bidi(&input.text, style, styles, direction);
        let manual = styles.iter().any(|s| s.word_break == WordBreak::Manual);
        self.styles(styles, bases, |_| breaks::PROFILE_BYTES)?;
        let boundary = 4 * size_of::<u32>() as u64 + size_of::<breaks::BreakOpportunity>() as u64;
        // G <= scalar_count + 1. Four u32 slots include the caret merge's
        // explicit left.len + right.len reservation before deduplication.
        self.processed(
            input,
            boundary.saturating_add(if first_line { 32 } else { 0 }),
            bases,
            |_, text| {
                let scalars = text.chars().count() as u64;
                let len = text.len() as u64;
                let mut bytes =
                    projection(text)
                        .saturating_add(scalars.saturating_mul(boundary))
                        .saturating_add(len) // retained byte levels, even ASCII
                        .saturating_add(scalars.saturating_mul(size_of::<
                            crate::analysis::bidi::BidiParagraph,
                        >() as u64))
                        .saturating_add(complex_scratch(text, false));
                if manual {
                    bytes = bytes.saturating_add(len);
                }
                if vertical {
                    bytes = bytes
                        .saturating_add(combine::ANALYSIS_ITEM_BYTES)
                        .saturating_add(len); // upright view
                }
                if bidi {
                    bytes = bytes
                        .saturating_add(len.saturating_mul(BIDI_SCRATCH_PER_BYTE))
                        .saturating_add(len); // optional same-width U+2028 normalization
                }
                if internal_bidi {
                    // TCY itemization + geometry each invoke bidi on disjoint span
                    // text. Also covers retained internal levels and per-scalar
                    // cluster/order/tab scratch; per-glyph paint remains glyph-bounded.
                    bytes = bytes.saturating_add(len.saturating_mul(3 * BIDI_SCRATCH_PER_BYTE));
                }
                if first_line {
                    // Units <= scalars + processed items. Future first-line cuts
                    // (two offsets/unit + G opportunities) and both cursor maps.
                    bytes = bytes.saturating_add(scalars.saturating_add(1).saturating_mul(32));
                }
                bytes
            },
        )
    }

    fn styles(
        &mut self,
        styles: &[InlineStyle],
        bases: &mut BaseScopes,
        bytes: impl Fn(&InlineStyle) -> u64,
    ) -> Result<(), LimitExceeded> {
        self.reserve(
            styles
                .iter()
                .fold(0u64, |sum, s| sum.saturating_add(bytes(s))),
        )?;
        bases.analysis_styles(styles, bytes)
    }

    fn processed(
        &mut self,
        input: &Processed,
        extra: u64,
        bases: &mut BaseScopes,
        bytes: impl Fn(&Item, &str) -> u64,
    ) -> Result<(), LimitExceeded> {
        let item_bytes = |item: &Item| {
            bytes(
                item,
                &input.text[item.text.start as usize..item.text.end as usize],
            )
        };
        self.reserve(
            input
                .items
                .iter()
                .fold(extra, |sum, item| sum.saturating_add(item_bytes(item))),
        )?;
        if bases.enabled() {
            for (i, item) in input.items.iter().enumerate() {
                bases.item(i, LimitKind::AnalysisBytes, item_bytes(item))?;
            }
        }
        Ok(())
    }
}

fn vertical(mode: WritingMode) -> bool {
    matches!(mode, WritingMode::VerticalRl | WritingMode::VerticalLr)
}

fn projection(text: &str) -> u64 {
    (text.len() as u64).saturating_add(
        (text.chars().count() as u64).saturating_mul(breaks::PROJECTION_SCALAR_BYTES),
    )
}

fn complex_scratch(text: &str, word: bool) -> u64 {
    let lb = CodePointMapData::<props::LineBreak>::new();
    text.chars().filter(|c| !c.is_ascii()).fold(0u64, |sum, c| {
        let bytes = match lb.get(c) {
            props::LineBreak::ComplexContext => SA_SCRATCH_PER_SCALAR,
            // WordSegmenter's complex-script table is not the line-break
            // SA/ID/CJ table: e.g. 々/ゝ/ゞ are NS but use its dictionary.
            // Reserve the dictionary/fallback allowance for every other
            // non-ASCII scalar in case context, without depending on a
            // private script classifier or dropping authored characters.
            _ if word => DICTIONARY_SCRATCH_PER_SCALAR,
            props::LineBreak::Ideographic | props::LineBreak::ConditionalJapaneseStarter => {
                DICTIONARY_SCRATCH_PER_SCALAR
            }
            _ => 0,
        };
        sum.saturating_add(bytes)
    })
}
