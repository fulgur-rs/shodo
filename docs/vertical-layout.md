# Vertical glyph output

`ParagraphStyle::writing_mode` supports horizontal-tb, vertical-rl/lr and
sideways-rl/lr. Vertical modes resolve `InlineStyle::text_orientation` as mixed,
upright or sideways; sideways writing modes rotate the horizontal text flow.
Mixed uses Unicode vertical orientation per grapheme. Upright text uses vertical
glyph substitutions and advances, with font vertical origins applied once.
The default vertical feature is `vert`; explicit `font_features` can override it
or request `vrt2`. Font variation coordinates also apply to vertical metrics.

Layout, selection and caret coordinates remain logical inline/block coordinates.
To paint an accepted glyph, use the retained face, actual `font_size` and
`normalized_coords`, then transform its outline through the public output:

```rust,ignore
let converter = PhysicalConverter::new(
    line.writing_mode(), line.used_direction(), container_size,
);
let (inline, block) = run.glyph_origin(glyph_index).unwrap();
let origin = converter.point(inline, line.block_offset() + block);
let matrix = run.glyph_transform();
let x_axis = converter.vector(matrix.inline_x, matrix.block_x);
let y_axis = converter.vector(matrix.inline_y, matrix.block_y);
```

The matrix maps local right/down axes. For a Skrifa outline with y pointing up,
negate the second column once when constructing the raster transform. Add the
canvas inset to `origin`. The matrix compensates for the converter's inline
direction so RTL text retains the glyph's physical orientation. `glyph_origin`
includes the original shaping advance where RTL cell positioning needs it;
CSS tracking or justification must not be added again. The fixture renderer in
`dev/fixtures/examples/support/glyph_paint.rs` demonstrates the complete path.
Use `PhysicalConverter::rect` for boxes and selections, and `logical_point` for
incoming physical hit coordinates.

`text-orientation: upright` changes the used direction to LTR. Computed RTL
remains available to inherited horizontal content; use `Line::used_direction`
when constructing the converter. The line-over side is the physical right for
both vertical modes, so vertical-lr baseline displacements have the opposite
logical block sign. `Line::metrics` exposes text-over/under positions, and
`Line::baseline(BaselineKind::Central)` exposes the central baseline.

Mixed text aligns clockwise Latin runs by converting their actual horizontal
font metrics from an alphabetic origin to the central baseline. Inline changes
between mixed and sideways also convert the selected baseline once; text-edge
alignment uses the converted extents. Horizontal and sideways writing modes
retain their alphabetic baseline behavior.

## Combined text

`TextCombineUpright::All` creates a horizontal composition in vertical-rl/lr.
The external advance and measured square are one computed em. Internal glyph
advances retain their actual horizontal shaping values; the public matrix adds
compression and positioning within the square. Internal tracking and forced
breaks are ignored, while word spacing and preserved tab stops participate in
the horizontal composition. The composition is indivisible for line breaking;
source ownership, caret cuts and selection remain available inside it.

For multiple typographic units, fullwidth forms are narrowed before compression,
including directly authored fullwidth text and voiced Katakana. A single unit
keeps its fullwidth form. Shaping uses Unicode 16.0 inverse width mappings while
`Line::text()` and the processed/source byte ranges retain the original text.

`Line::text_combinations()` returns each processed source range and its logical
square once, including compositions consisting only of preserved tabs. Add
`line.block_offset()` to the square before converting it. Place an emphasis mark
once per square; internal `Cluster::flags.emphasis_excluded` prevents duplicate
marks. Compression preserves each glyph's source owner. Box boundaries and the
CSS lookaround rules determine which inherited `all` sequences can combine.
Only `None` and `All` are exposed; digit-count variants and ruby are not provided.

## Fixed images

The snapshot matrix includes 20 vertical/sideways cases covering both directions,
mixed/upright/sideways orientation and combined text with two paint owners. Its
geometry includes physical glyph origins, public matrices and composition
squares. The original 26 horizontal PNGs and geometry are byte-identical.

```sh
cargo run -p shodo-fixtures --example snapshots -- --output target/vertical-report
```

Use a fresh report directory for each run. Expectations are generated from fixed
accepted glyph output and checked with zero decoded-pixel/geometry tolerance;
they are not browser conformance results. Tests additionally compare asymmetric
real outlines with literal physical rotation/compression matrices.

The [executed verification record](verification/vertical-final-gates.json) lists
the exact commands and results for the implementation. Stable and Rust 1.89.0
each passed 653 workspace tests, with 46 exact snapshot matches. The record also
includes feature, allocator, wasm, font reproduction and measured benchmark checks.
