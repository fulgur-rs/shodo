# Integration guide

Start with the [README](../README.md) for a minimal fixed-width example. This guide
covers font configuration, incremental layout, retained output, resource budgets
and hit testing. All examples use public shodo APIs.

The caller supplies computed styles, atomic sizes and float placement, and paints
the resulting fragments. shodo lays out one paragraph; page layout, DOM/CSS
resolution and renderer policy belong to the application.

## Font collections

`FontCollection::new` lazily enumerates system fonts on the first query. Use
`FontCollection::with_options` with `system_fonts: false` for deterministic bundled
fonts. `for_document` creates an isolated layer for CSS faces; `FontQuery` selects
one font for a supplied grapheme. `set_generic_families` and
`set_fallback_families` configure shared deterministic family and script/locale
mappings. `resolve_ch` and `resolve_ic` return the selected face and pixel advance.

If no registered or fallback face covers a grapheme, layout warns and emits
deterministic glyph 0 with a 1em base advance and zero advance for combining
marks/default ignorables. This fallback is not a drawable substitute for font data.

`register` accepts structurally checked sfnt/TTC. `register_face` selects one face
and attaches `FontFaceDescriptor`. `register_sources` tries full/PostScript local
names and downloaded font data in order. WOFF/WOFF2 decoding is also exposed as
`decode_web_font`. URL fetching belongs to the caller. See the executable and
file-based examples in the `shodo::font` module documentation.

Default features are `system-fonts`, `web-fonts`, and `complex-scripts`. Disable default features for
bundled sfnt-only applications; font matching, metrics, and shaping still work. Add `features = ["complex-scripts"]` to retain dictionary segmentation without system/web font support. The
system backend is available only on supported native platforms, while wasm builds
use memory-backed fonts. Linux builds with `system-fonts` require Fontconfig
development files and pkg-config (on Debian/Ubuntu, `libfontconfig1-dev`).
Compressed input, decoded streams and returned font blobs
are checked against font budgets; transformed WOFF2 reconstruction may temporarily
allocate beyond the retained-blob budget. `wuff` rejects reconstructed output above
128 MiB. Cache entries and per-face font-cache work have separate limits.

## Usage

Build a paragraph and lay it out one line at a time at a given width. Glyphs and advances come from the matched fonts.

```rust
use shodo::font::FontCollection;
use shodo::limits::Limits;
use shodo::style::{InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult, RichText};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let limits = Limits::default();
    let fonts = FontCollection::new(&limits);
    let mut cx = LayoutContext::new();
    let style = InlineStyle {
        font_size: 16.0,
        ..InlineStyle::default()
    };
    let paragraph_style = ParagraphStyle {
        root: style.clone(),
        ..ParagraphStyle::default()
    };

    let paragraph = RichText::with_limits(&paragraph_style, &limits)
        .push("Hello world from shodo", &style)
        .build(&mut cx, &fonts)?;

    let options = LineOptions::default();
    let mut constraint = LineConstraint::new(160.0);
    let mut token = paragraph.start_token();

    loop {
        match paragraph.next_line(
            &mut cx,
            token,
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) {
            LineResult::Line(line) => {
                println!("{}", &line.text()[line.text_range()]);
                token = line.break_token();
                constraint.block_offset += line.block_size();
            }
            LineResult::Done => break,
            other => return Err(format!("Unexpected layout result: {other:?}").into()),
        }
    }

    Ok(())
}
```

Once built, a `Paragraph` is immutable, and clones share its data. As long as the text, paragraph style, inline styles, and font generations remain unchanged, you can reuse the paragraph with different widths and line layout constraints. A `BreakToken` is valid only for the paragraph that produced it.

A `BreakPlan` is ignored when its paragraph, width, options, atomic revision, or
float constraints do not match.

For fixed-width content without float placement, the loop can be replaced with:

```rust,ignore
let lines = paragraph.break_all(&mut cx, &options, 160.0, &AtomicSizes::EMPTY);
for line in &lines {
    println!("{}", &line.text()[line.text_range()]);
}
```

`break_all` keeps floats as zero-width anchors and splits at block boundaries. For incremental float layout, retain the cursor across lines, retry from `line_start`, and withdraw displaced floats in reverse order, one at a time. Re-reported withdrawn floats must be deferred for that line. Accept only lines with no displaced floats, and save/restore token, cursor, placements, deferred floats, and withdrawal records together when discarding lookahead or moving to another page.

`lines` handles `FloatEncountered` and `BlockSizeExceeded` through its constraint callback. The callback must change the constraint to make progress; retry an over-tall first-page line with `max_block_size: None`. `LayoutContext::shrink_to(0)` releases the retained partial line, shaping scratch, and plans.

For DOM integration, pass `NodeId` and `TextSource` values to `ParagraphBuilder`. The caller computes sizes and baselines for images and other atomic inlines and supplies them through `AtomicSizes`. Rendering is also the caller's responsibility; use `Line::fragments()` to read the layout output.
Ruby readings are retained child Lines; compose their transforms before physical
conversion and keep their source datasets separate. See the [ruby contract](ruby.md).

Read build warnings through `Paragraph::warnings()` and line layout warnings through `LayoutContext::take_warnings()`. Resource limit violations are returned as `LimitExceeded`.

## Shaping data and budgets

A `GlyphRunView` exposes the actual font ID and retained `FontData`, effective size,
normalized variation coordinates, resolved variations, script, language, and
synthetic `embolden`/`skew` requests. Render the returned glyph IDs against that
face and instance; synthesis is a renderer request, not an outline transformation
performed by shodo. Runs retain their font layer after the collection is dropped.

Set `ParagraphStyle::first_line` by cloning the normal root and changing the
applicable properties. A descendant value equal to the normal root inherits the
first-line value; a differing descendant value is preserved. This legacy fallback cannot distinguish an equal explicitly declared child
value from an inherited value. Use `ParagraphBuilder::open_inline_with_first_line`
or `RichText::push_with_first_line` to supply the caller-resolved normal and
first-line styles of every inline; explicit alternatives use the supported
first-line properties exactly, without this inference. An explicit alternative
also activates first-line layout when the root override is absent. See the
[resolved input contract](first-line-style-contract.md) for examples,
property scope, migration and the pinned raikiri producer limitation. Only the first
formatted line uses the alternate set, including after a float retry; forced or
block boundaries discontinue it. `Line::text()` and `Line::offset_mapping()` expose
the chosen set, so slice that text with `Line::text_range()` rather than slicing
`Paragraph::text()` for a transformed first line.

Main and first-line processed text, items, styles, and glyphs share each build
budget. Default shaping runs are bounded by 64 KiB and paragraph glyph output by
2^22; checks precede copying into retained glyph storage. A giant grapheme or tiny
run budget warns and makes scalar-level shaping progress while preserving the
original grapheme as an indivisible layout unit. A run's pen is bounded to ±2^30
layout units. Line-edge windows are bounded to 4096 UTF-8 bytes; cuts whose prefix
or continuation cannot be safely reshaped preserve the whole shared cluster and
warn. The line's owned windows also share the glyph-output budget.

Each line asks for at most 64 windows' worth of edge reshape bytes
(`max_reshape_window_bytes` x 64, 256 KiB by default), charged per request rather
than per cache miss so the result never depends on what an earlier layout left in
the context; past it the line keeps shared glyphs and warns.

Each `LayoutContext` is `Send` and deliberately not `Sync`: use one per thread.
It reuses shaping scratch and at most 64 font-qualified shaping plans, and keeps
the reshaped line-edge windows of the most recent paragraph (at most 256 entries
and about 32K glyph-equivalents, roughly 0.9 MiB; a single window costing more
than 1K glyph-equivalents is not retained); no word cache is retained.
`shrink_to(bytes)` drops all plans and that window cache and caps the combined
accounted scratch/partial-line buffers, preferring scratch when it fits. It does
not measure separately shared paragraph/font allocations. Shaping also releases
scratch exceeding the current run budget's conservative storage bound. See the
[measurement record](shaping-measurements.md) and
[unsent harfrust status proposal](harfrust-shaping-status-proposal.md).

## Horizontal output and hit testing

`GlyphRunView::metrics()` uses the same face, effective size and variation
coordinates as shaping. `Line::metrics()` separates the final line-box extents
from the root font's text-over/text-under edges. Every participating run can
contribute to line height; block padding/borders remain paint geometry.
`Line::overflow_rect()` returns nominal glyph ink and painted box bounds in
line-local coordinates. Add `block_offset` before physical conversion. Renderer
strokes, antialiasing and decoration effects can extend those nominal bounds.
`hang_start()` and `hang_end()` report excluded punctuation advances and eligible
trailing whitespace. See the [Japanese layout contract](japanese-layout.md) for
trimming, hanging and justification policies.

Tracking uses the visual neighbors' half spacing, with no outer half at either
line edge. Word spacing and justification affect layout advances while keeping
natural `Cluster::shaping_advance` available. Cursive runs use word-level tracking
fallback without elongation or arbitrary inter-letter gaps. Tabs measure the root
font's space/ch with root spacing, even inside differently sized text.
Inter-character justification counts legal typographic boundaries inside retained
ligatures and aggregates their expansion onto the final glyph cluster. Combining
continuations and indivisible transforms stay closed. An unexpandable justified
line uses `text-align-last`; a `justify` fallback centers it. `JustifyAll` implies
last-line justification, including this fallback when expansion is disabled.
`TextAutospace::Normal` adds one eighth of the containing inline's actual ic
between eligible visual CJK/letter/digit neighbors. Intervening box edges block
it, and a soft wrap removes the boundary gap. `Cluster::source_char` and `flags`
describe processed source; a displayed synthetic hyphen retains U+00AD. Emphasis
is placed per typographic character by a renderer, not once per shaping cluster.

`hit::LineLayout::new(&lines)` indexes the finalized, accepted lines once and
borrows them. Each `TextPosition` identifies a line, a UTF-8 offset in that
line's `text()`, and an `Affinity`. First-line transforms can make datasets
differ; translate through that line's optional `offset_mapping()` when DOM
positions are needed. Mapping is not required for geometric queries. At a bidi
boundary, upstream/downstream positions can have different visual locations.
Interior bytes, graphemes and indivisible transforms snap to legal boundaries.
GDEF ligature carets use actual size/variation; absent, invalid or unsupported
contour-point data falls back to proportional grapheme positions.

`caret`, `hit_test` and `selection_rects` return logical geometry with line block
offsets already applied. Convert through `PhysicalConverter` for painting; glyph
fragments themselves still have line-local block positions. Outside hits clamp
with `inside=false`; NaN and empty layouts return `None`. Logical movement skips
duplicate affinities at one source offset; visual movement retains distinct
locations and skips coincident stops. Selection preserves bidi gaps and merges
only touching equal-height regions on the same line. These APIs do not perform
editing or IME operations.

Accepted `Line`s own their paragraph/font data, so they can outlive the original
handles. Relayout and justification never mutate previously accepted lines.
See the executable [horizontal hit example](../examples/horizontal_hit.rs) and the
[contract coverage](horizontal-layout-contracts.md):

```sh
cargo run --example horizontal_hit -- path/to/a-font.ttf
# Without a font path, the example uses deterministic missing-font fallback.
cargo run --example horizontal_hit --no-default-features
```
