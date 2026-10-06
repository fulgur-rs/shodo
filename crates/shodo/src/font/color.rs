//! Color glyph formats carried by a face.

use peniko::FontData;
use skrifa::raw::TableProvider;

/// Color glyph tables a face carries, for renderers choosing a paint path.
///
/// shodo does not paint glyphs. Use these flags to choose a paint backend for
/// a face, then read the glyph data from [`FontData`] yourself. A face can
/// carry several formats and can mix color and outline glyphs: COLR, CBDT,
/// sbix and SVG each cover only the glyph IDs they list. Check the glyph ID in
/// the chosen table, and fall back to the outline when it is absent.
///
/// A flag is set only when the table header parses and lists at least one
/// glyph. The flags do not check every record, so a renderer must still
/// handle malformed glyph data. Font matching ranks faces by table presence
/// alone, so a face can win color presentation matching with every flag here
/// unset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ColorGlyphFormats {
    /// COLR version 0 base glyph records (layers of solid CPAL colors).
    pub colr_v0: bool,
    /// COLR version 1 base glyph paint records (paint graphs).
    pub colr_v1: bool,
    /// CBDT bitmap data with its CBLC location table.
    pub cbdt: bool,
    /// sbix bitmap strikes.
    pub sbix: bool,
    /// SVG glyph documents.
    pub svg: bool,
}

impl ColorGlyphFormats {
    /// Reads the table headers of a face. Costs a few table directory
    /// lookups and allocates nothing; cache the result per
    /// [`super::FontId`] when calling it for many runs. Returns no formats
    /// when the data is not a readable font.
    pub fn from_font_data(data: &FontData) -> Self {
        skrifa::FontRef::from_index(data.data.as_ref(), data.index)
            .map(|font| Self::from_font(&font))
            .unwrap_or_default()
    }

    pub(crate) fn from_font(font: &skrifa::FontRef<'_>) -> Self {
        let colr = font.colr().ok();
        let colr_v0 = colr
            .as_ref()
            .is_some_and(|colr| colr.num_base_glyph_records() > 0);
        let colr_v1 = colr.as_ref().is_some_and(|colr| {
            colr.version() >= 1
                && colr
                    .base_glyph_list()
                    .and_then(Result::ok)
                    .is_some_and(|list| list.num_base_glyph_paint_records() > 0)
        });
        let cbdt = font.cbdt().is_ok() && font.cblc().is_ok_and(|cblc| cblc.num_sizes() > 0);
        let sbix = font.sbix().is_ok_and(|sbix| sbix.num_strikes() > 0);
        let svg = font
            .svg()
            .ok()
            .and_then(|svg| svg.svg_document_list().ok())
            .is_some_and(|list| list.num_entries() > 0);
        Self {
            colr_v0,
            colr_v1,
            cbdt,
            sbix,
            svg,
        }
    }

    /// Whether the face carries no color glyph table.
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::ColorGlyphFormats;
    use crate::font::sfnt::build_sfnt;
    use peniko::{Blob, FontData};

    fn formats(tables: &[([u8; 4], Vec<u8>)]) -> ColorGlyphFormats {
        let data = FontData::new(Blob::from(build_sfnt(tables)), 0);
        ColorGlyphFormats::from_font_data(&data)
    }

    fn be(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    /// COLR v0 header with `records` base glyph records and no layers.
    fn colr_v0(records: u16) -> Vec<u8> {
        let mut table = be(&[
            &0u16.to_be_bytes(),
            &records.to_be_bytes(),
            &14u32.to_be_bytes(),
            &0u32.to_be_bytes(),
            &0u16.to_be_bytes(),
        ]);
        for gid in 0..records {
            table.extend(be(&[
                &gid.to_be_bytes(),
                &0u16.to_be_bytes(),
                &0u16.to_be_bytes(),
            ]));
        }
        table
    }

    /// COLR v1 header with no v0 records and a BaseGlyphList of `records`.
    fn colr_v1(records: u32) -> Vec<u8> {
        let mut table = be(&[
            &1u16.to_be_bytes(),
            &0u16.to_be_bytes(),
            &0u32.to_be_bytes(),
            &0u32.to_be_bytes(),
            &0u16.to_be_bytes(),
            &34u32.to_be_bytes(),
            &[0u8; 16],
        ]);
        table.extend(records.to_be_bytes());
        for gid in 0..records as u16 {
            table.extend(be(&[&gid.to_be_bytes(), &0u32.to_be_bytes()]));
        }
        table
    }

    fn maxp(glyphs: u16) -> ([u8; 4], Vec<u8>) {
        (
            *b"maxp",
            be(&[&0x0000_5000u32.to_be_bytes(), &glyphs.to_be_bytes()]),
        )
    }

    #[test]
    fn plain_face_has_no_formats() {
        assert!(formats(&[]).is_empty());
        let latin = FontData::new(Blob::from(crate::test_support::fonts::LATIN.to_vec()), 0);
        assert!(ColorGlyphFormats::from_font_data(&latin).is_empty());
    }

    #[test]
    fn unreadable_data_has_no_formats() {
        let data = FontData::new(Blob::from(b"not a font".to_vec()), 0);
        assert!(ColorGlyphFormats::from_font_data(&data).is_empty());
        let data = FontData::new(Blob::from(build_sfnt(&[(*b"COLR", colr_v0(1))])), 3);
        assert!(ColorGlyphFormats::from_font_data(&data).is_empty());
    }

    #[test]
    fn colr_versions_report_their_records() {
        let v0 = formats(&[(*b"COLR", colr_v0(1))]);
        assert_eq!(
            v0,
            ColorGlyphFormats {
                colr_v0: true,
                ..Default::default()
            }
        );
        let v1 = formats(&[(*b"COLR", colr_v1(1))]);
        assert_eq!(
            v1,
            ColorGlyphFormats {
                colr_v1: true,
                ..Default::default()
            }
        );
        // Empty record lists and truncated headers list no color glyphs.
        assert!(formats(&[(*b"COLR", colr_v0(0))]).is_empty());
        assert!(formats(&[(*b"COLR", colr_v1(0))]).is_empty());
        assert!(formats(&[(*b"COLR", vec![0, 0, 0])]).is_empty());
    }

    #[test]
    fn bitmap_tables_need_their_index() {
        let cbdt = (*b"CBDT", be(&[&3u16.to_be_bytes(), &0u16.to_be_bytes()]));
        let mut cblc = be(&[
            &3u16.to_be_bytes(),
            &0u16.to_be_bytes(),
            &1u32.to_be_bytes(),
        ]);
        cblc.extend([0u8; 48]);
        let cblc = (*b"CBLC", cblc);
        assert!(formats(&[cbdt.clone(), cblc]).cbdt);
        // CBDT data without its location table cannot be painted.
        assert!(formats(&[cbdt]).is_empty());

        let sbix = |strikes: u32| {
            let mut table = be(&[
                &1u16.to_be_bytes(),
                &1u16.to_be_bytes(),
                &strikes.to_be_bytes(),
            ]);
            let first = 8 + 4 * strikes;
            for strike in 0..strikes {
                table.extend((first + strike * 12).to_be_bytes());
            }
            for _ in 0..strikes {
                table.extend(be(&[&16u16.to_be_bytes(), &72u16.to_be_bytes(), &[0u8; 8]]));
            }
            (*b"sbix", table)
        };
        assert!(formats(&[maxp(1), sbix(1)]).sbix);
        assert!(formats(&[maxp(1), sbix(0)]).is_empty());
    }

    #[test]
    fn svg_needs_a_document_entry() {
        let svg = |entries: u16| {
            let mut table = be(&[
                &0u16.to_be_bytes(),
                &10u32.to_be_bytes(),
                &0u32.to_be_bytes(),
            ]);
            table.extend(entries.to_be_bytes());
            for _ in 0..entries {
                table.extend([0u8; 12]);
            }
            (*b"SVG ", table)
        };
        assert!(formats(&[svg(1)]).svg);
        assert!(formats(&[svg(0)]).is_empty());
    }

    #[test]
    fn real_color_emoji_fixture_reports_cbdt() {
        let emoji = FontData::new(
            Blob::from(crate::test_support::fonts::EMOJI_COLOR.to_vec()),
            0,
        );
        assert_eq!(
            ColorGlyphFormats::from_font_data(&emoji),
            ColorGlyphFormats {
                cbdt: true,
                ..Default::default()
            }
        );
    }
}
