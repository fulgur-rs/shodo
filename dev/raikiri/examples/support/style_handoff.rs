//! Ownership split for CSS that the frozen S4 style gate rejects as residual.
//! Resolved values stay with their owner; nothing is reset to initial, and no
//! painting, positioning or BFC work happens here.
use super::diagnostic;
use raikiri_style::{
    ComputedBackgroundSize, ComputedCssPosition, ComputedLength, ComputedLengthPercentageOrAuto,
    ComputedOutline, ComputedTextDecorationThickness, ComputedTextUnderlineOffset, ComputedValues,
    property as css,
};
use shodo::style::TextDecoration;

/// Who must consume a residual field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Float placement and clearance in the block formatting context.
    Flow,
    /// Positioned-box placement and stacking.
    Positioned,
    /// Box background, outline and clip painting.
    BoxPaint,
    /// Text decoration lines carried by `PaintStyle`.
    Decoration,
    /// Mapped to `LineOptions` by shodo-9an.1.
    HangingPunctuation,
    /// Tracked by shodo-3v2 (vertical text).
    VerticalText,
}

impl Owner {
    pub fn name(self) -> &'static str {
        match self {
            Self::Flow => "flow",
            Self::Positioned => "positioned",
            Self::BoxPaint => "box-paint",
            Self::Decoration => "decoration",
            Self::HangingPunctuation => "hanging-punctuation(shodo-9an.1)",
            Self::VerticalText => "vertical-text(shodo-3v2)",
        }
    }
}

/// Owner of every residual field this step or a sibling issue accounts for.
pub fn owner(field: &str) -> Option<Owner> {
    Some(match field {
        "float" | "clear" => Owner::Flow,
        "position" | "left" | "top" | "z_index" => Owner::Positioned,
        "background_color"
        | "background_image"
        | "background_position"
        | "background_repeat"
        | "background_size"
        | "outline"
        | "outline_offset"
        | "overflow" => Owner::BoxPaint,
        "text_decoration_line"
        | "text_decoration_style"
        | "text_decoration_color"
        | "text_decoration_thickness"
        | "text_underline_offset" => Owner::Decoration,
        "hanging_punctuation" => Owner::HangingPunctuation,
        "cssom_writing_mode" | "text_orientation" => Owner::VerticalText,
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub float: css::FloatValue,
    pub clear: css::ClearValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Positioned {
    pub position: css::PositionValue,
    pub left: ComputedLengthPercentageOrAuto,
    pub top: ComputedLengthPercentageOrAuto,
    pub z_index: css::ZIndexValue,
    /// `absolute` / `fixed`: outside normal IFC flow. Legitimate for a single
    /// paragraph, but a whole page must still place the box, not drop it.
    pub out_of_flow: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxPaint {
    pub background_color: css::CssColor,
    pub background_image: css::BackgroundImage,
    pub background_position: ComputedCssPosition,
    pub background_repeat: css::BackgroundRepeat,
    pub background_size: ComputedBackgroundSize,
    pub outline: ComputedOutline,
    pub outline_offset: ComputedLength,
    pub overflow: css::OverflowXY,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decoration {
    pub line: css::TextDecorationLine,
}

/// Resolved values per owner, plus every residual field found and its owner.
#[derive(Clone, Debug, PartialEq)]
pub struct Handoff {
    pub flow: Flow,
    pub positioned: Positioned,
    pub box_paint: BoxPaint,
    pub decoration: Decoration,
    pub residual: Vec<(String, Owner)>,
}

fn keep<T: Clone>(value: &T) -> T {
    value.clone()
}

/// Split the residual fields of `values`. Every field the frozen S4 gate would
/// reject must have an owner; otherwise this fails closed.
pub fn split(
    values: &ComputedValues,
    profile: diagnostic::InputProfile,
) -> Result<Handoff, String> {
    let prepared = diagnostic::prepare_input(values, profile);
    let mut residual = Vec::new();
    for difference in diagnostic::differences(&prepared) {
        let Some(owner) = owner(&difference.field) else {
            return Err(format!("unmapped: {}", difference.field));
        };
        residual.push((difference.field, owner));
    }
    Ok(Handoff {
        flow: Flow {
            float: keep(&values.float),
            clear: keep(&values.clear),
        },
        positioned: Positioned {
            position: keep(&values.position),
            left: keep(&values.left),
            top: keep(&values.top),
            z_index: keep(&values.z_index),
            out_of_flow: matches!(
                values.position,
                css::PositionValue::Absolute | css::PositionValue::Fixed
            ),
        },
        box_paint: BoxPaint {
            background_color: keep(&values.background_color),
            background_image: keep(&values.background_image),
            background_position: keep(&values.background_position),
            background_repeat: keep(&values.background_repeat),
            background_size: keep(&values.background_size),
            outline: keep(&values.outline),
            outline_offset: keep(&values.outline_offset),
            overflow: keep(&values.overflow),
        },
        decoration: Decoration {
            line: keep(&values.text_decoration_line),
        },
        residual,
    })
}

/// This node's own solid underline as the existing `PaintStyle.underline`
/// input. Propagating an ancestor's line to inline text descendants, and the
/// single drawing of a shared glyph, remain the caller's existing walk and
/// paint path (see `raikiri_contracts.rs`); nothing is drawn here.
pub fn solid_underline(values: &ComputedValues) -> Result<Option<TextDecoration>, String> {
    let line = &values.text_decoration_line;
    if line.overline || line.line_through || line.blink || line.spelling_error || line.grammar_error
    {
        return Err("only a solid underline can be handed to PaintStyle".into());
    }
    if !line.underline {
        return Ok(None);
    }
    if values.text_decoration_style != css::TextDecorationStyle::Solid {
        return Err("only a solid underline can be handed to PaintStyle".into());
    }
    let rgba = |c: css::CssColor| [c.r, c.g, c.b, c.a];
    let color = match values.text_decoration_color {
        css::TextDecorationColor::CurrentColor => rgba(keep(&values.color)),
        css::TextDecorationColor::Resolved(c) => rgba(c),
        _ => return Err("unsupported decoration color".into()),
    };
    let thickness = match values.text_decoration_thickness {
        ComputedTextDecorationThickness::Auto | ComputedTextDecorationThickness::FromFont => None,
        ComputedTextDecorationThickness::Length(v) => Some(v.0),
    };
    if values.text_underline_offset != ComputedTextUnderlineOffset::Auto {
        return Err("underline offset needs its own PaintStyle mapping".into());
    }
    Ok(Some(TextDecoration {
        color: Some(color),
        thickness,
        offset: None,
    }))
}
