//! Solid retained text paint over fixed fonts; no DOM or system fonts.
//! cargo run -p shodo-fixtures --example paint_styles -- /tmp/paint-styles.png
#[path = "support/glyph_paint.rs"]
mod glyph_paint;
use shodo::geometry::{Direction, WritingMode};
use shodo::style::{FontFamily, InlineStyle, PaintStyle, ParagraphStyle, TextDecoration};
use shodo::{AtomicSizes, LayoutContext, Line, RichText};
use shodo_fixtures::{FONTS, load_fonts};
const RED: [u8; 4] = [220, 20, 30, 255];
const BLUE: [u8; 4] = [20, 40, 220, 255];
const GREEN: [u8; 4] = [0, 160, 0, 255];
const YELLOW: [u8; 4] = [240, 180, 0, 255];
fn sample(mode: WritingMode, direction: Direction) -> Vec<Line> {
    let fonts = load_fonts(&Default::default()).unwrap();
    let mut cx = LayoutContext::new();
    let red = InlineStyle {
        font_size: 32.0,
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        paint: PaintStyle {
            color: RED,
            ..Default::default()
        },
        direction,
        ..Default::default()
    };
    let mut blue = red.clone();
    blue.paint.color = BLUE;
    blue.paint.underline = Some(TextDecoration {
        color: Some(GREEN),
        offset: Some(12.0),
        thickness: Some(2.0),
    });
    blue.paint.strikethrough = Some(TextDecoration {
        color: Some(YELLOW),
        offset: Some(-10.0),
        thickness: Some(2.0),
    });
    let mut translucent = red.clone();
    translucent.paint.color = [220, 20, 30, 128];
    let p = RichText::new(&ParagraphStyle {
        root: red.clone(),
        writing_mode: mode,
        direction,
        ..Default::default()
    })
    .push("f", &red)
    .push("fi", &blue)
    .push(" b ", &blue)
    .push("a", &translucent)
    .build(&mut cx, &fonts.collection)
    .unwrap();
    // Returning Line retains fonts, styles and accepted glyphs after all
    // paragraph/context/font owners above are dropped.
    p.break_all(&mut cx, &Default::default(), 120.0, &AtomicSizes::EMPTY)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "paint-styles.png".into());
    let lines = sample(WritingMode::HorizontalTb, Direction::Ltr);
    let (image, count) = glyph_paint::try_paint_styled_on_canvas(&lines, 320, 160)?;
    image.save_png(&path)?;
    println!("{path}: {count} accepted glyphs, retained colors and solid decorations");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use shodo::Fragment;
    fn count(lines: &[Line]) -> usize {
        lines
            .iter()
            .flat_map(|l| l.fragments())
            .filter_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r.glyphs().len())
                } else {
                    None
                }
            })
            .sum()
    }
    fn pixel(image: &tiny_skia::Pixmap, x: usize, y: usize) -> [u8; 4] {
        image.data()[4 * (y * image.width() as usize + x)..][..4]
            .try_into()
            .unwrap()
    }
    #[test]
    fn retained_paint_draws_colors_and_both_decorations() {
        let lines = sample(WritingMode::HorizontalTb, Direction::Ltr);
        let (image, n) = glyph_paint::try_paint_styled_on_canvas(&lines, 320, 160).unwrap();
        assert_eq!(n, count(&lines));
        let first = lines[0]
            .fragments()
            .find_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(first.text_range(), 0..3);
        assert_eq!(first.glyphs().len(), 1);
        assert_eq!(first.paint_style().color, RED);
        // Literal fixed-font baseline=34.203125, margin10. The second source
        // starts at ffi's GDEF315*32/1000+10=20.08. Explicit +12/-10 offsets
        // and2px strokes give center pixels y56/y34, independently of output
        // decoration helpers. x24 is inside the second source, x12 outside.
        assert_eq!(pixel(&image, 24, 56), GREEN);
        assert_eq!(pixel(&image, 12, 56), [255; 4]);
        assert_eq!(pixel(&image, 24, 34), YELLOW);
        let pixels = image.data().as_chunks::<4>().0;
        assert!(pixels.iter().filter(|p| **p == RED).count() > 20);
        assert!(pixels.iter().filter(|p| **p == BLUE).count() > 20);
        assert!(
            pixels.iter().any(|p| p[0] >= 235
                && p[0] <= 239
                && p[1] >= 134
                && p[1] <= 140
                && p[2] >= 138
                && p[2] <= 145
                && p[3] == 255),
            "alpha128 glyph blends once over white"
        );
        assert!(
            glyph_paint::try_paint_styled_on_canvas(&lines, 20, 20).is_err(),
            "clipped output must fail"
        );
    }
    #[test]
    fn rtl_and_vertical_colors_and_strokes_follow_public_coordinates() {
        for (mode, dir) in [
            (WritingMode::HorizontalTb, Direction::Rtl),
            (WritingMode::VerticalRl, Direction::Ltr),
            (WritingMode::VerticalLr, Direction::Ltr),
        ] {
            let lines = sample(mode, dir);
            let (image, n) = glyph_paint::try_paint_styled_on_canvas(&lines, 320, 240).unwrap();
            assert_eq!(n, count(&lines));
            let pixels = image.data().as_chunks::<4>().0;
            assert!(pixels.contains(&GREEN));
            assert!(pixels.contains(&YELLOW));
            assert!(pixels.contains(&RED));
            assert!(pixels.contains(&BLUE));
        }
    }
}
