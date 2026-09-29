# Public API PNG sample

Run from the repository root with an optional output path:

```sh
cargo +stable run --offline -p shodo-harness --example render_png -- /tmp/shodo-sample.png
```

Without the path, the example writes `target/shodo-sample.png` relative to the
current directory. It creates missing output directories and replaces that PNG.
It uses only the three checked-in fixture fonts; no OS discovery, external
font download or raikiri layout is needed. The first build needs Cargo's
normal dependency cache; omit `--offline` if the cache is not populated.

`dev/fixtures/examples/render_png.rs` shows:

- `ParagraphBuilder` with three text nodes forming `ffi`, Latin/Arabic styles,
  a forced break and a caller-sized atomic inline;
- `Paragraph::next_line`, accepted `BreakToken` continuation and each line's
  block offset, preserving the public layout output;
- direct `GlyphRunView::font_data` bytes/face index, effective font size,
  normalized variation coordinates, glyph IDs/positions and run baseline;
- the caller's owner-to-color map and the middle `ffi` source's partial
  annotation via offset mapping and `LineLayout::selection_rects`;
- a green atomic **border** rectangle, separate from its margin reservation,
  using the accepted line's block offset and fragment geometry.

The actual image contains a red shared `ffi`, a blue annotation under only its
middle source, a green atomic and a second Arabic line. The accepted output has
five glyphs (including the space), two lines and one atomic. The outline is
painted once in its first source owner's color; the middle source's blue color
does not duplicate that shared glyph. A source link can use that same partial
region; the example's annotation is not a complete CSS underline implementation.
See the [shared glyph contract](shared-glyph-contract.md) for boundary affinity,
link containment and source-to-owner distinctions.

`dev/fixtures/examples/support/glyph_paint.rs` is shared by the shared-glyph and
PNG regression tests, rendering examples, and snapshot harness. It takes
accepted lines and caller paint colors/annotations, without reshaping strings.
Tiny-Skia fills the actual Skrifa outlines with y-axis reversal at each returned
baseline. Glyph positions already include inline placement; the painter does
not add a second run origin. Atomic and annotation geometry add the line block
offset only where required by the public coordinate contract.

## Scope and failures

This is a minimal **horizontal fixed outline-font** sample, with an opaque white
512px canvas, 10px glyph padding, scale1 and unhinted antialiased outlines. It
supports the fixture TrueType/CFF outlines and reads each run's actual variation
coordinates. It does not quantize or adjust layout positions to hide differences.
Color/bitmap/SVG glyphs, full CSS painting, vertical layout and general fonts are
outside this sample. The CLI accepts a PNG path, not arbitrary font inputs.

Synthetic weight or skew requests return `UnsupportedSynthesis`; they are not
silently ignored. The fixed sample requests neither. A renderer that supports
those requests must apply them while honoring layout metrics. Missing font
bytes, invalid sfnt data, unavailable outlines or paint ownership and failed
canvas/annotation construction return `PaintError`. An unloaded collection's
internal .notdef stub has bytes but no drawable outline, so it produces
`MissingOutline`. PNG encoding/write failures propagate to the command's error
exit. Whitespace can have an empty outline without being an error.

The simple blue annotation has a fixed2px thickness below its source region;
it does not claim skip-ink, decoration propagation or CSS thickness/position
conformance. Inline backgrounds, borders, images, selection overlays and
out-of-flow content need separate caller painting. The sample contains only the
explicit supported glyphs and atomic, so omitting those other fragment types
is not a general page-rendering promise.

## Verification and reference

The shared fixture renderer also converts vertical glyph origins and matrices
through `PhysicalConverter`; see [vertical output](vertical-layout.md).
An asymmetric real `F` outline is compared against independent literal rotation
and compression matrices for 48 mode/orientation/direction/combine combinations.

`dev/fixtures/tests/png_render.rs` checks a30×20px atomic on the second accepted
line at its actual block offset, real glyph ink, unsupported synthetic weight
and the no-outline stub error. Existing shared-glyph tests check owner colors,
partial blue pixel bounds and split/unsplit Arabic RGBA equality. The example's
unit test checks retained fixture font data, glyph/atomic counts and deterministic
PNG encoding after paragraph/context/font collection owners have been dropped.

```sh
cargo +stable test --offline -p shodo-harness --test png_render
cargo +stable test --offline -p shodo-raikiri --test shared_glyph
cargo +stable test --offline -p shodo-harness --example render_png
```

The [Parley Tiny-Skia sample](https://github.com/linebender/parley/blob/main/examples/tiny_skia_render/src/main.rs)
is a useful reference for direct glyph-outline rendering. This example uses
shodo's own public glyph coordinates and caller-owned styles/atomics. No Parley
code or renderer dependency is added to the shodo library.
