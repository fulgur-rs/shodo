//! Neutral `ch` length: a factor of the declaring font's U+0030 advance.
use super::{FontCollection, FontQuery, FontUnit};

/// A CSS `ch` length bound to the font it was declared with. Callers keep the
/// declaring font for inherited values instead of re-resolving in a child font.
#[derive(Clone, Debug)]
pub struct ChLength {
    pub query: FontQuery,
    pub size: f32,
    pub factor: f32,
}

impl ChLength {
    /// Returns `factor` times the '0' advance of the face selected by `query`
    /// (CSS fallback 0.5em when no face has the glyph). `None` if `size` or
    /// `factor` is not finite or `size` is negative.
    pub fn resolve(&self, fonts: &FontCollection) -> Option<FontUnit> {
        if !(self.size.is_finite() && self.size >= 0. && self.factor.is_finite()) {
            return None;
        }
        let unit = fonts.resolve_ch(&self.query, self.size);
        Some(FontUnit {
            id: unit.id,
            advance: self.factor * unit.advance,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontOptions;
    use crate::limits::Limits;

    fn empty() -> FontCollection {
        FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        )
    }

    #[test]
    fn missing_zero_glyph_uses_half_em_times_factor() {
        let ch = ChLength {
            query: FontQuery::default(),
            size: 20.,
            factor: 3.,
        };
        let unit = ch.resolve(&empty()).unwrap();
        assert_eq!(unit.id, None);
        assert_eq!(unit.advance, 30.);
    }

    #[test]
    fn non_finite_or_negative_inputs_are_rejected() {
        let fonts = empty();
        for (size, factor) in [
            (f32::NAN, 1.),
            (20., f32::INFINITY),
            (20., f32::NAN),
            (-1., 1.),
        ] {
            let ch = ChLength {
                query: FontQuery::default(),
                size,
                factor,
            };
            assert!(ch.resolve(&fonts).is_none(), "{size} {factor}");
        }
    }
}
