//! Fixed-font caller example, shared by contract tests. It draws accepted
//! glyph IDs once; annotations use separate source rectangles. It is not a
//! general CSS painter (no skip-ink or decoration propagation).
//! Color-font support is CBDT/CBLC PNG only; other color formats need a backend.
#[path = "bitmap_paint.rs"]
pub(crate) mod bitmap_paint;
use shodo::geometry::{LogicalRect, PhysicalConverter, PhysicalSize, WritingMode};
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
    paint(lines, color, annotations, None, false)
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
    paint(lines, color, annotations, Some((width, height)), false)
}

/// Paint retained solid colors and source decorations using public logical
/// transforms, including horizontal RTL. Fixed-font example only; CSS
/// propagation and skip-ink remain caller-owned; this sample adds CBDT PNG drawing.
#[allow(dead_code)] // Other fixed snapshot callers retain their legacy colors.
pub fn try_paint_styled_on_canvas(
    lines: &[Line],
    width: u32,
    height: u32,
) -> Result<(tiny_skia::Pixmap, usize), PaintError> {
    paint(lines, |_| [0, 0, 0, 255], &[], Some((width, height)), true)
}

fn draw_decoration(
    image: &mut tiny_skia::Pixmap,
    line: &Line,
    decoration: shodo::DecorationRect,
) -> Result<(), PaintError> {
    // Source decoration rects already include line.block_offset().
    let rect = converter(line, image.width(), image.height()).rect(decoration.rect);
    let rect = tiny_skia::Rect::from_xywh(10.0 + rect.x, 10.0 + rect.y, rect.width, rect.height)
        .ok_or(PaintError::InvalidAnnotation)?;
    check_bounds(rect, image)?;
    let [r, g, b, a] = decoration.color;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(r, g, b, a);
    image.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
    Ok(())
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

fn converter(line: &Line, width: u32, height: u32) -> PhysicalConverter {
    PhysicalConverter::new(
        line.writing_mode(),
        line.used_direction(),
        PhysicalSize {
            width: width as f32 - 20.0,
            height: height as f32 - 20.0,
        },
    )
}

fn physical_rect(
    line: &Line,
    rect: LogicalRect,
    width: u32,
    height: u32,
) -> Option<tiny_skia::Rect> {
    if line.writing_mode() == WritingMode::HorizontalTb {
        tiny_skia::Rect::from_xywh(
            10.0 + rect.inline_start,
            10.0 + rect.block_start,
            rect.inline_size,
            rect.block_size,
        )
    } else {
        let rect = converter(line, width, height).rect(rect);
        tiny_skia::Rect::from_xywh(10.0 + rect.x, 10.0 + rect.y, rect.width, rect.height)
    }
}

fn paint(
    lines: &[Line],
    mut color: impl FnMut(NodeId) -> [u8; 4],
    annotations: &[LogicalRect],
    canvas: Option<(u32, u32)>,
    retained: bool,
) -> Result<(tiny_skia::Pixmap, usize), PaintError> {
    let (width, height) = canvas.unwrap_or_else(|| {
        if lines
            .first()
            .is_some_and(|line| line.writing_mode() != WritingMode::HorizontalTb)
        {
            return (
                lines
                    .iter()
                    .map(|l| l.block_offset() + l.block_size())
                    .fold(0.0, f32::max)
                    .ceil() as u32
                    + 40,
                lines
                    .iter()
                    .map(|l| l.inline_size())
                    .fold(0.0, f32::max)
                    .ceil() as u32
                    + 40,
            );
        }
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
        let spans = if retained {
            line.paint_spans()
        } else {
            Vec::new()
        };
        for span in &spans {
            if let Some(decoration) = span.underline() {
                draw_decoration(&mut image, line, decoration)?;
            }
        }
        for fragment in line.fragments() {
            if let Fragment::Atomic(atomic) = fragment {
                let mut rect = atomic.border_rect;
                rect.block_start += line.block_offset();
                if let Some(rect) = physical_rect(line, rect, width, height) {
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
            let [r, g, b, a] = if retained {
                run.paint_style().color
            } else {
                color(run.node().ok_or(PaintError::MissingOwner)?)
            };
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(r, g, b, a);
            for (index, glyph) in run.glyphs().enumerate() {
                let bitmap_transform =
                    if !retained && line.writing_mode() == WritingMode::HorizontalTb {
                        tiny_skia::Transform::from_translate(
                            10.0 + glyph.inline_position,
                            10.0 + line.block_offset() + run.baseline() + glyph.block_offset,
                        )
                    } else {
                        let converter = converter(line, width, height);
                        let (inline, block) =
                            run.glyph_origin(index).ok_or(PaintError::InvalidBitmap)?;
                        let (x, y) = converter.point(inline, line.block_offset() + block);
                        let matrix = run.glyph_transform();
                        let (xx, xy) = converter.vector(matrix.inline_x, matrix.block_x);
                        let (yx, yy) = converter.vector(matrix.inline_y, matrix.block_y);
                        // Bitmap coordinates already use y-down, unlike outlines.
                        tiny_skia::Transform::from_row(xx, xy, yx, yy, 10.0 + x, 10.0 + y)
                    };
                if bitmap_paint::paint_bitmap(
                    &font,
                    GlyphId::new(glyph.id),
                    run.font_size(),
                    bitmap_transform,
                    &mut image,
                    canvas.is_some(),
                )? {
                    count += 1;
                    continue;
                }
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
                    let transform = if !retained && line.writing_mode() == WritingMode::HorizontalTb
                    {
                        // Preserve the established horizontal sample coordinates.
                        tiny_skia::Transform::from_row(
                            1.0,
                            0.0,
                            0.0,
                            -1.0,
                            10.0 + glyph.inline_position,
                            10.0 + line.block_offset() + run.baseline() + glyph.block_offset,
                        )
                    } else {
                        let converter = converter(line, width, height);
                        let (inline, block) =
                            run.glyph_origin(index).ok_or(PaintError::MissingOutline)?;
                        let (x, y) = converter.point(inline, line.block_offset() + block);
                        let matrix = run.glyph_transform();
                        let (xx, xy) = converter.vector(matrix.inline_x, matrix.block_x);
                        let (yx, yy) = converter.vector(matrix.inline_y, matrix.block_y);
                        // skrifa outlines use y-up; the public matrix uses y-down.
                        tiny_skia::Transform::from_row(xx, xy, -yx, -yy, 10.0 + x, 10.0 + y)
                    };
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
        for span in &spans {
            if let Some(decoration) = span.strikethrough() {
                draw_decoration(&mut image, line, decoration)?;
            }
        }
    }
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(0, 0, 255, 255);
    for rect in annotations {
        let rect = if let Some(line) = lines.first() {
            physical_rect(
                line,
                LogicalRect {
                    inline_start: rect.inline_start,
                    inline_size: rect.inline_size,
                    block_start: rect.block_start + rect.block_size + 3.0,
                    block_size: 2.0,
                },
                width,
                height,
            )
        } else {
            tiny_skia::Rect::from_xywh(
                10.0 + rect.inline_start,
                13.0 + rect.block_start + rect.block_size,
                rect.inline_size,
                2.0,
            )
        }
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
    InvalidBitmap,
    UnsupportedBitmap,
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
            Self::InvalidBitmap => "bitmap data, dimensions or placement are invalid",
            Self::UnsupportedBitmap => "example only supports CBDT/CBLC PNG with top-left bearings",
            Self::UnsupportedSynthesis => "example does not support synthetic weight or skew",
        })
    }
}
impl std::error::Error for PaintError {}
