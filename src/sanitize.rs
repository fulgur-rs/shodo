//! Normalization of caller-supplied `f32` values at build time, so that line
//! layout and the output views only ever see finite, in-range numbers.
//! Replaced values are reported through the bounded warning sink.

use crate::builder::RawItem;
use crate::limits::{WarningKind, WarningSink};
use crate::node::{InlineEdges, Sides};
use crate::style::{InlineStyle, LineHeight, TabSize, VerticalAlign};

/// Largest accepted magnitude of a length that may be negative (spacing,
/// margins, `vertical-align` lengths); larger values are clamped.
const MAX_LENGTH: f32 = 1.0e7;

/// Initial values of `font-weight` and `font-width` (CSS Fonts 4).
const INITIAL_FONT_WEIGHT: f32 = 400.0;
const INITIAL_FONT_WIDTH: f32 = 100.0;

fn finite_or(v: f32, fallback: f32, what: &str, warnings: &mut WarningSink) -> f32 {
    if v.is_finite() {
        v
    } else {
        warnings.push(
            WarningKind::NonFiniteInput,
            format!("non-finite {what} replaced with {fallback}"),
        );
        fallback
    }
}

/// Finite value clamped to `±MAX_LENGTH`.
fn length(v: f32, what: &str, warnings: &mut WarningSink) -> f32 {
    let v = finite_or(v, 0.0, what, warnings);
    if v.abs() > MAX_LENGTH {
        warnings.push(WarningKind::Saturated, format!("{what} clamped to 1e7 px"));
        v.clamp(-MAX_LENGTH, MAX_LENGTH)
    } else {
        v
    }
}

/// Finite, non-negative value clamped to `MAX_LENGTH`.
fn non_negative_length(v: f32, what: &str, warnings: &mut WarningSink) -> f32 {
    let v = length(v, what, warnings);
    if v < 0.0 {
        warnings.push(
            WarningKind::NegativeInput,
            format!("negative {what} replaced with 0"),
        );
        0.0
    } else {
        v
    }
}

/// Finite and non-negative, without an upper clamp (the value is scaled or
/// converted with saturation later).
fn non_negative(v: f32, what: &str, warnings: &mut WarningSink) -> f32 {
    let v = finite_or(v, 0.0, what, warnings);
    if v < 0.0 {
        warnings.push(
            WarningKind::NegativeInput,
            format!("negative {what} replaced with 0"),
        );
        0.0
    } else {
        v
    }
}

/// Sanitizes every float of an interned style except `font_size`, which the
/// paragraph clamps separately.
pub(crate) fn style(s: &mut InlineStyle, warnings: &mut WarningSink) {
    s.line_height = match s.line_height {
        LineHeight::Normal => LineHeight::Normal,
        LineHeight::Px(v) => LineHeight::Px(non_negative(v, "line-height", warnings)),
        LineHeight::Number(n) => LineHeight::Number(non_negative(n, "line-height", warnings)),
    };
    s.letter_spacing = length(s.letter_spacing, "letter-spacing", warnings);
    s.word_spacing = length(s.word_spacing, "word-spacing", warnings);
    s.tab_size = match s.tab_size {
        TabSize::Spaces(n) => TabSize::Spaces(non_negative(n, "tab-size", warnings)),
        TabSize::Px(v) => TabSize::Px(non_negative(v, "tab-size", warnings)),
    };
    if let VerticalAlign::Length(v) = s.vertical_align {
        s.vertical_align = VerticalAlign::Length(length(v, "vertical-align", warnings));
    }
    s.font_weight = finite_or(s.font_weight, INITIAL_FONT_WEIGHT, "font-weight", warnings);
    s.font_width = finite_or(s.font_width, INITIAL_FONT_WIDTH, "font-width", warnings);
}

fn sides(s: &mut Sides, what: &str, allow_negative: bool, warnings: &mut WarningSink) {
    for v in [
        &mut s.inline_start,
        &mut s.inline_end,
        &mut s.block_start,
        &mut s.block_end,
    ] {
        *v = if allow_negative {
            length(*v, what, warnings)
        } else {
            non_negative_length(*v, what, warnings)
        };
    }
}

/// Margins may be negative (CSS 2.1 §8.3); padding and border widths may not
/// (CSS 2.1 §8.4, §8.5.1).
pub(crate) fn edges(e: &mut InlineEdges, warnings: &mut WarningSink) {
    sides(&mut e.margin, "margin", true, warnings);
    sides(&mut e.border, "border width", false, warnings);
    sides(&mut e.padding, "padding", false, warnings);
}

pub(crate) fn items(items: &mut [RawItem], warnings: &mut WarningSink) {
    for item in items {
        if let RawItem::Open { edges: e, .. } | RawItem::Atomic { edges: e, .. } = item {
            edges(e, warnings);
        }
    }
}
