//! Fixed-font caller example, shared by contract tests. It draws accepted
//! glyph IDs once; annotations use separate source rectangles. It is not a
//! general CSS painter (no skip-ink, decoration propagation or color fonts).
use shodo::geometry::LogicalRect;
use shodo::node::NodeId;
use shodo::{Fragment, Line};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
};
struct Pen(tiny_skia::PathBuilder);
impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x: f32, y: f32, z: f32, w: f32) {
        self.0.quad_to(x, y, z, w);
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.0.cubic_to(a, b, c, d, e, f);
    }
    fn close(&mut self) {
        self.0.close();
    }
}
pub fn paint(
    lines: &[Line],
    mut color: impl FnMut(NodeId) -> [u8; 4],
    annotations: &[LogicalRect],
) -> (tiny_skia::Pixmap, usize) {
    let height = lines
        .iter()
        .map(|l| l.block_offset() + l.block_size())
        .fold(0.0, f32::max)
        .ceil() as u32
        + 40;
    let mut image = tiny_skia::Pixmap::new(512, height).unwrap();
    image.fill(tiny_skia::Color::WHITE);
    let mut count = 0;
    for line in lines {
        for fragment in line.fragments() {
            let Fragment::GlyphRun(run) = fragment else {
                continue;
            };
            assert!(
                !run.embolden() && run.skew().is_none(),
                "example supports unsynthesized outline fixtures"
            );
            let data = run.font_data().unwrap();
            let font = FontRef::from_index(data.data.as_ref(), data.index).unwrap();
            let [r, g, b, a] = color(run.node().expect("fixture must supply paint owners"));
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(r, g, b, a);
            for glyph in run.glyphs() {
                let outline = font.outline_glyphs().get(GlyphId::new(glyph.id)).unwrap();
                let mut pen = Pen(tiny_skia::PathBuilder::new());
                outline
                    .draw(
                        DrawSettings::unhinted(
                            Size::new(run.font_size()),
                            LocationRef::new(run.normalized_coords()),
                        ),
                        &mut pen,
                    )
                    .unwrap();
                if let Some(path) = pen.0.finish() {
                    image.fill_path(
                        &path,
                        &paint,
                        tiny_skia::FillRule::Winding,
                        tiny_skia::Transform::from_row(
                            1.0,
                            0.0,
                            0.0,
                            -1.0,
                            10.0 + glyph.inline_position,
                            10.0 + line.block_offset() + run.baseline() + glyph.block_offset,
                        ),
                        None,
                    );
                }
                count += 1;
            }
        }
    }
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(0, 0, 255, 255);
    for rect in annotations {
        let rect = tiny_skia::Rect::from_xywh(
            10.0 + rect.inline_start,
            13.0 + rect.block_start + rect.block_size,
            rect.inline_size,
            2.0,
        )
        .unwrap();
        image.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
    }
    (image, count)
}
