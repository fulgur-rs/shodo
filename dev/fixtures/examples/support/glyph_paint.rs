//! Fixed-font caller example, shared by contract tests. It draws accepted
//! glyph IDs once, traversing retained ruby lanes with their transforms. It is not a
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
    space: LineSpace<'_>,
    decoration: shodo::DecorationRect,
) -> Result<(), PaintError> {
    // Source decoration rects already include line.block_offset().
    let rect = mapped_rect(space, decoration.rect, image.width(), image.height(), false)
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
        count += paint_line(
            &mut image,
            line,
            LineSpace {
                root: line,
                to_parent: tiny_skia::Transform::identity(),
                nested: false,
            },
            PaintOptions {
                strict: canvas.is_some(),
                retained,
            },
            &mut color,
        )?;
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

#[derive(Clone, Copy)]
struct LineSpace<'a> {
    root: &'a Line,
    to_parent: tiny_skia::Transform,
    nested: bool,
}
#[derive(Clone, Copy)]
struct PaintOptions {
    strict: bool,
    retained: bool,
}

fn mapped_rect(
    space: LineSpace<'_>,
    rect: LogicalRect,
    width: u32,
    height: u32,
    legacy: bool,
) -> Option<tiny_skia::Rect> {
    let mapped = tiny_skia::Rect::from_xywh(
        rect.inline_start,
        rect.block_start,
        rect.inline_size,
        rect.block_size,
    )?
    .transform(space.to_parent)?;
    let rect = LogicalRect {
        inline_start: mapped.x(),
        block_start: mapped.y(),
        inline_size: mapped.width(),
        block_size: mapped.height(),
    };
    if legacy {
        return physical_rect(space.root, rect, width, height);
    }
    let physical = converter(space.root, width, height).rect(rect);
    tiny_skia::Rect::from_xywh(
        10.0 + physical.x,
        10.0 + physical.y,
        physical.width,
        physical.height,
    )
}

#[allow(clippy::too_many_arguments)]
fn glyph_to_canvas(
    run: shodo::GlyphRunView<'_>,
    index: usize,
    line: &Line,
    space: LineSpace<'_>,
    width: u32,
    height: u32,
    retained: bool,
) -> Result<tiny_skia::Transform, PaintError> {
    if !retained && !space.nested && line.writing_mode() == WritingMode::HorizontalTb {
        let glyph = run.glyphs().get(index).ok_or(PaintError::InvalidBitmap)?;
        return Ok(tiny_skia::Transform::from_translate(
            10.0 + glyph.inline_position,
            10.0 + line.block_offset() + run.baseline() + glyph.block_offset,
        ));
    }
    let (inline, block) = run.glyph_origin(index).ok_or(PaintError::InvalidBitmap)?;
    let matrix = run.glyph_transform();
    let logical = tiny_skia::Transform::from_row(
        matrix.inline_x,
        matrix.block_x,
        matrix.inline_y,
        matrix.block_y,
        inline,
        line.block_offset() + block,
    );
    let composed = space.to_parent.pre_concat(logical);
    let physical = converter(space.root, width, height);
    let (x, y) = physical.point(composed.tx, composed.ty);
    let (xx, xy) = physical.vector(composed.sx, composed.ky);
    let (yx, yy) = physical.vector(composed.kx, composed.sy);
    Ok(tiny_skia::Transform::from_row(
        xx,
        xy,
        yx,
        yy,
        10.0 + x,
        10.0 + y,
    ))
}

fn paint_line(
    image: &mut tiny_skia::Pixmap,
    line: &Line,
    space: LineSpace<'_>,
    options: PaintOptions,
    color: &mut impl FnMut(NodeId) -> [u8; 4],
) -> Result<usize, PaintError> {
    let width = image.width();
    let height = image.height();
    let retained = options.retained;
    let mut count = 0;
    let spans = if retained {
        line.paint_spans()
    } else {
        Vec::new()
    };
    for span in &spans {
        if let Some(decoration) = span.underline() {
            draw_decoration(image, space, decoration)?;
        }
    }
    for fragment in line.fragments() {
        if let Fragment::RubyAnnotation(a) = fragment {
            if a.visibility() == shodo::RubyVisibility::Visible {
                let t = a.transform();
                let child_space = LineSpace {
                    root: space.root,
                    to_parent: space
                        .to_parent
                        .pre_concat(tiny_skia::Transform::from_translate(
                            0.0,
                            line.block_offset(),
                        ))
                        .pre_concat(tiny_skia::Transform::from_row(
                            t.inline_inline,
                            t.block_inline,
                            t.inline_block,
                            t.block_block,
                            t.inline_offset,
                            t.block_offset,
                        )),
                    nested: true,
                };
                count += paint_line(image, a.line(), child_space, options, color)?;
            }
            continue;
        }
        if let Fragment::Atomic(atomic) = fragment {
            let mut rect = atomic.border_rect;
            rect.block_start += line.block_offset();
            if let Some(rect) = mapped_rect(space, rect, width, height, !retained && !space.nested)
            {
                if options.strict {
                    check_bounds(rect, image)?;
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
                glyph_to_canvas(run, index, line, space, width, height, retained)?;
            if bitmap_paint::paint_bitmap(
                &font,
                GlyphId::new(glyph.id),
                run.font_size(),
                bitmap_transform,
                image,
                options.strict,
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
                // skrifa outlines use y-up; bitmaps and public matrices use y-down.
                let transform =
                    bitmap_transform.pre_concat(tiny_skia::Transform::from_scale(1.0, -1.0));
                if options.strict {
                    let bounds = path
                        .clone()
                        .transform(transform)
                        .and_then(|path| path.compute_tight_bounds())
                        .ok_or(PaintError::InvalidCanvas)?;
                    check_bounds(bounds, image)?;
                }
                image.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
            }
            count += 1;
        }
    }
    for span in &spans {
        if let Some(decoration) = span.strikethrough() {
            draw_decoration(image, space, decoration)?;
        }
    }
    Ok(count)
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
