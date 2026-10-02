//! Zero-advance glyphs can still paint ink; ignore controls only after shaping.
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};
use shodo_fixtures::FONTS;
use shodo_harness::glyph_paint;
use skrifa::{FontRef, MetadataProvider};

// Give space an actual X outline, so Harfrust's default space substitution
// cannot accidentally satisfy the no-ink contract. Keep the pinned outlines.
fn font_with_visible_space() -> Vec<u8> {
    let original = FONTS[0].bytes;
    let face = FontRef::from_index(original, 0).unwrap();
    let glyph = face.charmap().map('X').unwrap().to_u32();
    let chars = [' ', 'X', '\u{34f}', '\u{180e}', '\u{2060}'];
    let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12];
    cmap.extend_from_slice(&12u16.to_be_bytes());
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&(16u32 + chars.len() as u32 * 12).to_be_bytes());
    cmap.extend_from_slice(&0u32.to_be_bytes());
    cmap.extend_from_slice(&(chars.len() as u32).to_be_bytes());
    for ch in chars {
        cmap.extend_from_slice(&(ch as u32).to_be_bytes());
        cmap.extend_from_slice(&(ch as u32).to_be_bytes());
        cmap.extend_from_slice(&glyph.to_be_bytes());
    }
    let mut bytes = original.to_vec();
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    for index in 0..count {
        let at = 12 + index * 16;
        if &bytes[at..at + 4] == b"cmap" {
            let start = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
            assert!(cmap.len() <= len);
            bytes[start..start + cmap.len()].copy_from_slice(&cmap);
            bytes[at + 12..at + 16].copy_from_slice(&(cmap.len() as u32).to_be_bytes());
            return bytes;
        }
    }
    panic!("pinned font has no cmap");
}

#[test]
fn default_ignorables_paint_no_ink_even_with_a_visible_space_glyph() {
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
            font_with_visible_space(),
            0,
            FontFaceDescriptor {
                family: "Visible Space".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("Visible Space".into())],
            font_size: 20.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let render = |text: &str| {
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
        let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        assert_eq!(paragraph.text(), text);
        let lines = paragraph.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
        let glyphs = lines
            .iter()
            .flat_map(|line| line.fragments())
            .filter_map(|f| match f {
                Fragment::GlyphRun(run) => Some(run.glyphs().len()),
                _ => None,
            })
            .sum::<usize>();
        let (image, _) =
            glyph_paint::try_paint_on_canvas(&lines, |_| [0, 160, 0, 255], &[], 128, 96).unwrap();
        (image, glyphs)
    };
    for ch in ['\u{2060}', '\u{180e}', '\u{34f}'] {
        for (text, reference, count) in [
            (format!("XX{ch}XX"), "XXXX", 4),
            (format!("XX {ch}XX"), "XX XX", 5),
        ] {
            let (expected, _) = render(reference);
            assert!(
                expected
                    .data()
                    .chunks_exact(4)
                    .any(|pixel| pixel[..3] != [255, 255, 255])
            );
            let (actual, glyphs) = render(&text);
            assert!(actual.data() == expected.data(), "{text:?}: extra ink");
            assert_eq!(glyphs, count, "{text:?}: extra glyphs");
        }
    }
}
