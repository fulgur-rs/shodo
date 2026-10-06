//! raikiri → shodo mapping for vertical writing values.
//!
//! Callers must read `ComputedValues::cssom_writing_mode`: the renderer-facing
//! `writing_mode` field is normalized to horizontal, so reading it silently
//! drops vertical text. Every raikiri enum here is `#[non_exhaustive]`, so an
//! unknown value is an error rather than a default.
use raikiri_style::property as r;
use shodo::geometry::WritingMode;
use shodo::style::{TextAutospace, TextCombineUpright, TextOrientation};

pub fn writing_mode(mode: r::WritingMode) -> Result<WritingMode, String> {
    match mode {
        r::WritingMode::HorizontalTb => Ok(WritingMode::HorizontalTb),
        r::WritingMode::VerticalRl => Ok(WritingMode::VerticalRl),
        r::WritingMode::VerticalLr => Ok(WritingMode::VerticalLr),
        r::WritingMode::SidewaysRl => Ok(WritingMode::SidewaysRl),
        r::WritingMode::SidewaysLr => Ok(WritingMode::SidewaysLr),
        _ => Err("writing-mode unsupported".into()),
    }
}

pub fn text_orientation(orientation: r::TextOrientation) -> Result<TextOrientation, String> {
    match orientation {
        r::TextOrientation::Mixed => Ok(TextOrientation::Mixed),
        r::TextOrientation::Upright => Ok(TextOrientation::Upright),
        r::TextOrientation::Sideways => Ok(TextOrientation::Sideways),
        _ => Err("text-orientation unsupported".into()),
    }
}

pub fn text_combine_upright(value: r::TextCombineUpright) -> Result<TextCombineUpright, String> {
    match value {
        r::TextCombineUpright::None => Ok(TextCombineUpright::None),
        r::TextCombineUpright::All => Ok(TextCombineUpright::All),
        _ => Err("text-combine-upright unsupported".into()),
    }
}

/// Preserve the selected classes; only insert behavior is supported.
pub fn text_autospace(value: r::TextAutospace) -> Result<TextAutospace, String> {
    match value {
        r::TextAutospace::Normal => Ok(TextAutospace::Normal),
        r::TextAutospace::Auto => Ok(TextAutospace::Auto),
        r::TextAutospace::NoAutospace => Ok(TextAutospace::NoAutospace),
        r::TextAutospace::Custom {
            ideograph_alpha,
            ideograph_numeric,
            punctuation,
            mode,
        } => match mode {
            r::TextAutospaceMode::None | r::TextAutospaceMode::Insert => {
                Ok(TextAutospace::Custom {
                    ideograph_alpha,
                    ideograph_numeric,
                    punctuation,
                })
            }
            r::TextAutospaceMode::Replace => Err("text-autospace replace unsupported".into()),
            _ => Err("text-autospace mode unsupported".into()),
        },
        _ => Err("text-autospace value unsupported".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_modes_map_one_to_one() {
        for (from, to) in [
            (r::WritingMode::HorizontalTb, WritingMode::HorizontalTb),
            (r::WritingMode::VerticalRl, WritingMode::VerticalRl),
            (r::WritingMode::VerticalLr, WritingMode::VerticalLr),
            (r::WritingMode::SidewaysRl, WritingMode::SidewaysRl),
            (r::WritingMode::SidewaysLr, WritingMode::SidewaysLr),
        ] {
            assert_eq!(writing_mode(from), Ok(to));
        }
    }

    #[test]
    fn orientations_and_combine_map_one_to_one() {
        assert_eq!(
            text_orientation(r::TextOrientation::Mixed),
            Ok(TextOrientation::Mixed)
        );
        assert_eq!(
            text_orientation(r::TextOrientation::Upright),
            Ok(TextOrientation::Upright)
        );
        assert_eq!(
            text_orientation(r::TextOrientation::Sideways),
            Ok(TextOrientation::Sideways)
        );
        assert_eq!(
            text_combine_upright(r::TextCombineUpright::None),
            Ok(TextCombineUpright::None)
        );
        assert_eq!(
            text_combine_upright(r::TextCombineUpright::All),
            Ok(TextCombineUpright::All)
        );
    }

    #[test]
    fn autospace_keywords_and_classes_map_without_accepting_replace() {
        assert_eq!(
            text_autospace(r::TextAutospace::Normal),
            Ok(TextAutospace::Normal)
        );
        assert_eq!(
            text_autospace(r::TextAutospace::NoAutospace),
            Ok(TextAutospace::NoAutospace)
        );
        assert_eq!(
            text_autospace(r::TextAutospace::Auto),
            Ok(TextAutospace::Auto)
        );
        for mask in 0..8 {
            for mode in [
                r::TextAutospaceMode::None,
                r::TextAutospaceMode::Insert,
                r::TextAutospaceMode::Replace,
            ] {
                let (ideograph_alpha, ideograph_numeric, punctuation) =
                    (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0);
                let actual = text_autospace(r::TextAutospace::Custom {
                    ideograph_alpha,
                    ideograph_numeric,
                    punctuation,
                    mode,
                });
                if mode == r::TextAutospaceMode::Replace {
                    assert!(actual.unwrap_err().contains("replace"));
                } else {
                    assert_eq!(
                        actual,
                        Ok(TextAutospace::Custom {
                            ideograph_alpha,
                            ideograph_numeric,
                            punctuation
                        })
                    );
                }
            }
        }
    }
}
