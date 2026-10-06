//! Color glyph format hints for renderers.

mod common;

use shodo::font::{ColorGlyphFormats, FontCollection, FontFaceDescriptor, FontOptions};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};

const LATIN: &[u8] = include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf");
const EMOJI: &[u8] = include_bytes!("../../../dev/fixtures/assets/fonts/emoji-color.ttf");

fn register(fonts: &FontCollection, data: &[u8], family: &str) -> shodo::font::FontId {
    fonts
        .register_face(
            data.to_vec(),
            0,
            FontFaceDescriptor {
                family: family.into(),
                ..Default::default()
            },
        )
        .unwrap()
}

#[test]
fn runs_and_collections_report_the_face_formats() {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let latin = register(&fonts, LATIN, "Fixture Latin");
    let emoji = register(&fonts, EMOJI, "Fixture Emoji");
    let cbdt = ColorGlyphFormats {
        cbdt: true,
        ..Default::default()
    };
    assert_eq!(
        fonts.color_glyph_formats(latin),
        Some(ColorGlyphFormats::default())
    );
    assert_eq!(fonts.color_glyph_formats(emoji), Some(cbdt));

    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![
                FontFamily::Named("Fixture Latin".into()),
                FontFamily::Named("Fixture Emoji".into()),
            ],
            font_size: 16.0,
            ..InlineStyle::default()
        },
        ..ParagraphStyle::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "a\u{1F600}",
    );
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let line = common::first_line(
        &paragraph,
        1000.0,
        &LineOptions::default(),
        &AtomicSizes::default(),
    );
    let runs: Vec<_> = line
        .fragments()
        .filter_map(|fragment| match fragment {
            Fragment::GlyphRun(run) => Some((run.font(), run.color_glyph_formats())),
            _ => None,
        })
        .collect();
    assert_eq!(runs, [(latin, ColorGlyphFormats::default()), (emoji, cbdt)]);
}
