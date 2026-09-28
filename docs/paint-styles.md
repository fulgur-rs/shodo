# Retained text paint

`InlineStyle::paint` retains resolved solid text color, underline and strike-through.
Both `ParagraphBuilder` and `RichText` accept it; no DOM or external style lookup
is needed. Colors are non-premultiplied sRGB `[r,g,b,a]` bytes. The default is
opaque black with no decorations.

```rust
use shodo::style::{InlineStyle, PaintStyle, TextDecoration};
let style = InlineStyle {
    paint: PaintStyle {
        color: [200, 20, 30, 255],
        underline: Some(TextDecoration {
            color: Some([0, 120, 0, 255]),
            offset: Some(3.0),
            thickness: Some(2.0),
        }),
        strikethrough: Some(TextDecoration::default()),
    },
    ..Default::default()
};
```

A missing decoration color uses the source's text color. Missing offset and
thickness use the actual selected font instance's underline/strike-through
metrics, including fallback, font-size-adjust and variations. Offsets are signed
px toward line-under from the alphabetic baseline; strike-through metrics usually
have negative offsets. Upright vertical text uses its central baseline as the
anchor for this metric-based policy. Explicit offsets can override it; this is
not CSS's automatic underline-position heuristic. Thickness is nonnegative px;
zero produces no decoration rectangle. Nonfinite optional lengths fall back to
font metrics with warnings, negative thickness becomes zero and finite lengths
clamp to ±1e7px (thickness to0..1e7).

## Glyph paint and source decoration

Draw each glyph from `Line::fragments()` exactly once with
`GlyphRunView::paint_style().color`. Color and decoration differences do not cut
shaping: an ffi ligature or joined Arabic sequence can span several source items.
A shared cluster's glyph belongs to its first scalar's source, which supplies its
paint. Retained paint does not change glyph advances, wrapping, intrinsic sizes
or line metrics. Fonts and paint remain owned by accepted `Line` data after the
builder, paragraph, layout context and original font collection are dropped.

`Line::paint_spans()` separately returns source-ordered `PaintSpan` values with
`node`, processed UTF-8 `text_range`, `style`, final logical `rect`, actual
`font`/`font_size` and `metrics`. This splits a shared ligature's source regions
using the existing GDEF/proportional caret geometry. An indivisible grapheme or
transform spanning several sources can return coincident rectangles, without
creating an internal caret. These regions are decoration inputs, not additional
glyph paint commands. Atomic inlines are excluded.

`PaintSpan::underline()` and `strikethrough()` resolve a `DecorationRect` with
solid color and geometry. Positions include `Line::block_offset()` and final
bidi, line wrapping and justification. Retained tabs use their source style's
primary font metrics and accepted containing-inline baseline. Removed whitespace
bytes are absent; positive surviving hanging advances remain selectable and
appear in the regions, including collapsed trailing spaces. Zero-width collapsed
regions are omitted. A caller can cache the spans: each call builds a caret index.

Decoration rectangles are centered on baseline+offset, with thickness across
that axis. Horizontal/sideways text decorates along inline; vertical-lr reverses
line-under in logical block coordinates. Combined text decorates along its
internal horizontal block axis, using the accepted composition baseline and
scaled source widths. Convert with `PhysicalConverter` using the accepted line's
`writing_mode()` and `used_direction()`. Do not add block offset twice. Pixel
rounding, clipping and antialiasing belong to the painter. Decoration overflow is
not included in `Line::ink_bounds()` and does not enlarge line height.

The accepted first-line dataset retains its corresponding paint. Exact
caller-resolved normal/first-line pairs work through
`open_inline_with_first_line` / `RichText::push_with_first_line`; the legacy
root override compares color, underline and strike-through independently.

## Caller policy and example

This API carries resolved per-source paint. The caller performs CSS color
inheritance and decorating-box propagation. It also decides line-edge whitespace
clipping, padding/atomic bridging, ancestor decoration overlap, skip-ink,
underline-position heuristics, overlines, wavy/dashed/double strokes, shadows and
color-font rasterization. A child with `underline: None` has no resolved source
underline; that alone is not a CSS decoration cancellation rule. Color-font
palettes and embedded images are distinct from this text color.

This boundary follows the distinction between decorating-box propagation and
painting in [CSS Text Decoration3](https://www.w3.org/TR/2022/CRD-css-text-decor-3-20220505/#line-decoration).
It is not a complete CSS decoration or WPT conformance claim.

Run the fixed-font, DOM-independent solid painter:

```sh
cargo run -p shodo-fixtures --example paint_styles -- /tmp/paint-styles.png
```

The red ffi is drawn once even though its later source is blue and has green
underline and yellow strike-through. A later blue letter and translucent red
letter demonstrate retained run colors. The example applies public glyph origin/
outline transforms and source rectangles directly, draws underlines before glyphs
and strike-through after them, and rejects clipping on its declared canvas. Its
tests check literal fixed-font pixel positions, accepted glyph counts, alpha
blending, ownership after layout owners drop, horizontal RTL and vertical-rl/lr.
It uses unhinted outlines from the pinned monochrome fixtures and solid strokes;
it does not implement skip-ink or general color-font rendering.
