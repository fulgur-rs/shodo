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
#[allow(dead_code)] // Standalone samples use the dynamic canvas wrapper.
pub fn try_paint(
    lines: &[Line],
    color: impl FnMut(NodeId) -> [u8; 4],
    annotations: &[LogicalRect],
) -> Result<(tiny_skia::Pixmap, usize), PaintError> {
    paint(lines, color, annotations, None)
}

/// Draw directly onto a declared canvas, rejecting any clipped outline or rect.
#[allow(dead_code)] // The fixed snapshot caller uses this wrapper.
pub fn try_paint_on_canvas(
    lines: &[Line],
    color: impl FnMut(NodeId) -> [u8; 4],
    annotations: &[LogicalRect],
    width: u32,
    height: u32,
) -> Result<(tiny_skia::Pixmap, usize), PaintError> {
    paint(lines, color, annotations, Some((width, height)))
}

fn check_bounds(rect: tiny_skia::Rect, image: &tiny_skia::Pixmap) -> Result<(), PaintError> {
    if rect.left() < 0.0
        || rect.top() < 0.0
        || rect.right() > image.width() as f32
        || rect.bottom() > image.height() as f32
    {
        Err(PaintError::ClippedOutput)
    } else {
        Ok(())
    }
}

fn paint(
    lines: &[Line],
    mut color: impl FnMut(NodeId) -> [u8; 4],
    annotations: &[LogicalRect],
    canvas: Option<(u32, u32)>,
) -> Result<(tiny_skia::Pixmap, usize), PaintError> {
    let (width, height) = canvas.unwrap_or_else(|| {
        (
            512,
            lines
                .iter()
                .map(|l| l.block_offset() + l.block_size())
                .fold(0.0, f32::max)
                .ceil() as u32
                + 40,
        )
    });
    let mut image = tiny_skia::Pixmap::new(width, height).ok_or(PaintError::InvalidCanvas)?;
    image.fill(tiny_skia::Color::WHITE);
    let mut count = 0;
    for line in lines {
        for fragment in line.fragments() {
            if let Fragment::Atomic(atomic) = fragment {
                let rect = atomic.border_rect;
                if let Some(rect) = tiny_skia::Rect::from_xywh(
                    10.0 + rect.inline_start,
                    10.0 + line.block_offset() + rect.block_start,
                    rect.inline_size,
                    rect.block_size,
                ) {
                    if canvas.is_some() {
                        check_bounds(rect, &image)?;
                    }
                    let [r, g, b, a] = color(atomic.node);
                    let mut paint = tiny_skia::Paint::default();
                    paint.set_color_rgba8(r, g, b, a);
                    image.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
                }
                continue;
            }
            let Fragment::GlyphRun(run) = fragment else {
                continue;
            };
            if run.embolden() || run.skew().is_some() {
                return Err(PaintError::UnsupportedSynthesis);
            }
            let data = run.font_data().ok_or(PaintError::MissingFont)?;
            let font = FontRef::from_index(data.data.as_ref(), data.index)
                .map_err(|_| PaintError::InvalidFont)?;
            let [r, g, b, a] = color(run.node().ok_or(PaintError::MissingOwner)?);
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(r, g, b, a);
            for glyph in run.glyphs() {
                let outline = font
                    .outline_glyphs()
                    .get(GlyphId::new(glyph.id))
                    .ok_or(PaintError::MissingOutline)?;
                let mut pen = Pen(tiny_skia::PathBuilder::new());
                outline
                    .draw(
                        DrawSettings::unhinted(
                            Size::new(run.font_size()),
                            LocationRef::new(run.normalized_coords()),
                        ),
                        &mut pen,
                    )
                    .map_err(|_| PaintError::MissingOutline)?;
                if let Some(path) = pen.0.finish() {
                    let transform = tiny_skia::Transform::from_row(
                        1.0,
                        0.0,
                        0.0,
                        -1.0,
                        10.0 + glyph.inline_position,
                        10.0 + line.block_offset() + run.baseline() + glyph.block_offset,
                    );
                    if canvas.is_some() {
                        let bounds = path
                            .clone()
                            .transform(transform)
                            .and_then(|path| path.compute_tight_bounds())
                            .ok_or(PaintError::InvalidCanvas)?;
                        check_bounds(bounds, &image)?;
                    }
                    image.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
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
        .ok_or(PaintError::InvalidAnnotation)?;
        if canvas.is_some() {
            check_bounds(rect, &image)?;
        }
        image.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
    }
    Ok((image, count))
}

#[derive(Debug, PartialEq, Eq)]
pub enum PaintError {
    ClippedOutput,
    InvalidCanvas,
    InvalidAnnotation,
    MissingOwner,
    MissingFont,
    InvalidFont,
    MissingOutline,
    UnsupportedSynthesis,
}
impl std::fmt::Display for PaintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ClippedOutput => "accepted outline or rectangle exceeds the drawing canvas",
            Self::InvalidCanvas => "cannot allocate PNG canvas",
            Self::InvalidAnnotation => "source annotation has no positive rectangle",
            Self::MissingOwner => "paint style needs a source owner",
            Self::MissingFont => "accepted glyph font bytes are unavailable",
            Self::InvalidFont => "accepted font data cannot be read",
            Self::MissingOutline => "glyph has no supported outline",
            Self::UnsupportedSynthesis => "example does not support synthetic weight or skew",
        })
    }
}
impl std::error::Error for PaintError {}
