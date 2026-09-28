//! Caller-owned CBDT/CBLC PNG rasterization of accepted glyphs. No source text.
use super::PaintError;
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    bitmap::{BitmapData, BitmapFormat, BitmapGlyph, BitmapStrikes, Origin},
    instance::Size,
};

pub fn paint_bitmap(
    font: &FontRef<'_>,
    id: GlyphId,
    size: f32,
    transform: tiny_skia::Transform,
    image: &mut tiny_skia::Pixmap,
    strict: bool,
) -> Result<bool, PaintError> {
    let Some(strikes) = BitmapStrikes::with_format(font, BitmapFormat::Cbdt) else {
        return Ok(false);
    };
    let Some(bitmap) = strikes.glyph_for_size(Size::new(size), id) else {
        // Bitmap-only fonts need no outline for their nominal blank glyph.
        // Do not infer that any zero-advance glyph is blank: marks can have ink.
        return Ok(font.charmap().map(' ').is_some_and(|space| space == id));
    };
    paint_bitmap_glyph(&bitmap, size, transform, image, strict)?;
    Ok(true)
}

pub fn paint_bitmap_glyph(
    bitmap: &BitmapGlyph<'_>,
    size: f32,
    transform: tiny_skia::Transform,
    image: &mut tiny_skia::Pixmap,
    strict: bool,
) -> Result<(), PaintError> {
    let BitmapData::Png(data) = bitmap.data else {
        return Err(PaintError::UnsupportedBitmap);
    };
    if ![
        size,
        bitmap.ppem_x,
        bitmap.ppem_y,
        bitmap.bearing_x,
        bitmap.bearing_y,
        bitmap.inner_bearing_x,
        bitmap.inner_bearing_y,
    ]
    .iter()
    .all(|v| v.is_finite())
        || size <= 0.
        || bitmap.ppem_x <= 0.
        || bitmap.ppem_y <= 0.
        || !transform.is_valid()
    {
        return Err(PaintError::InvalidBitmap);
    }
    // This backend accepts CBDT: top-left placement and no outer font-unit bearing.
    if bitmap.placement_origin != Origin::TopLeft
        || bitmap.bearing_x != 0.
        || bitmap.bearing_y != 0.
    {
        return Err(PaintError::UnsupportedBitmap);
    }
    // Validate IHDR dimensions against font metrics BEFORE decode allocates output.
    // A glyph raster is bounded to1M pixels (4MiBRGBA); real fixtures are136x128.
    if data.len() < 33
        || &data[..8] != b"\x89PNG\r\n\x1a\n"
        || &data[8..16] != b"\0\0\0\rIHDR"
        || bitmap.width == 0
        || bitmap.height == 0
        || bitmap
            .width
            .checked_mul(bitmap.height)
            .is_none_or(|n| n > 1_048_576)
        || u32::from_be_bytes(data[16..20].try_into().unwrap()) != bitmap.width
        || u32::from_be_bytes(data[20..24].try_into().unwrap()) != bitmap.height
    {
        return Err(PaintError::InvalidBitmap);
    }
    let sx = size / bitmap.ppem_x;
    let sy = size / bitmap.ppem_y;
    let local = tiny_skia::Transform::from_row(
        sx,
        0.,
        0.,
        sy,
        bitmap.inner_bearing_x * sx,
        -bitmap.inner_bearing_y * sy,
    );
    let placed = transform.pre_concat(local);
    let bounds = tiny_skia::Rect::from_xywh(0., 0., bitmap.width as f32, bitmap.height as f32)
        .and_then(|r| r.transform(placed))
        .ok_or(PaintError::InvalidBitmap)?;
    if !placed.is_valid() {
        return Err(PaintError::InvalidBitmap);
    }
    if strict {
        super::check_bounds(bounds, image)?;
    }
    // tiny-skia expands PNG colors to RGBA8 and premultiplies alpha once.
    let decoded = tiny_skia::Pixmap::decode_png(data).map_err(|_| PaintError::InvalidBitmap)?;
    if decoded.width() != bitmap.width || decoded.height() != bitmap.height {
        return Err(PaintError::InvalidBitmap);
    }
    image.draw_pixmap(
        0,
        0,
        decoded.as_ref(),
        &tiny_skia::PixmapPaint {
            quality: tiny_skia::FilterQuality::Bilinear,
            ..Default::default()
        },
        placed,
        None,
    );
    Ok(())
}
