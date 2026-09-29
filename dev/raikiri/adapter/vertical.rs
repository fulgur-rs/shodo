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

/// Only `normal` and `no-autospace` have a shodo equivalent; `auto` and
/// explicit boundary sets are rejected instead of being treated as `normal`.
pub fn text_autospace(value: r::TextAutospace) -> Result<TextAutospace, String> {
    match value {
        r::TextAutospace::Normal => Ok(TextAutospace::Normal),
        r::TextAutospace::NoAutospace => Ok(TextAutospace::NoAutospace),
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
    fn autospace_auto_and_custom_are_rejected() {
        assert_eq!(
            text_autospace(r::TextAutospace::Normal),
            Ok(TextAutospace::Normal)
        );
        assert_eq!(
            text_autospace(r::TextAutospace::NoAutospace),
            Ok(TextAutospace::NoAutospace)
        );
        assert!(text_autospace(r::TextAutospace::Auto).is_err());
        assert!(
            text_autospace(r::TextAutospace::Custom {
                ideograph_alpha: true,
                ideograph_numeric: true,
                punctuation: true,
                mode: r::TextAutospaceMode::None,
            })
            .is_err()
        );
    }
}
