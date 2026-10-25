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

Collection clones share caches across workers. Cached cluster matches (including
missing matches), `ch`/`ic` values and shaping data use shared read access without
waiting for the font catalog lock. Hits preserve LRU recency; misses update the
bounded caches under exclusive access, and font registration still invalidates
matching and unit values through the shared/document generations.

Use `FontCollection::generations()` as the font dependency in a paragraph reuse
key. It returns `(shared_generation, None)` for a shared collection and
`(shared_generation, Some(document_generation))` for a document. Successful
registration increments the owning layer. `set_generic_families` and
`set_fallback_families` increment the shared counter, even when called through
a document; its local `generation()` stays unchanged. Comparing the full pair
therefore catches shared policy changes as well as document font registration.
Compare counters within the same collection or its clones. A reuse key spanning
different collections also needs collection identity from `layer_handle().id()`.

If no registered or fallback face covers a grapheme, layout warns and emits
deterministic glyph 0 with a 1em base advance and zero advance for combining
marks/default ignorables. This fallback is not a drawable substitute for font data.

`register` accepts structurally checked sfnt/TTC. `register_face` selects one face
and attaches `FontFaceDescriptor`. `register_sources` tries full/PostScript local
names and downloaded font data in order. WOFF/WOFF2 decoding is also exposed as
`decode_web_font`. URL fetching belongs to the caller. See the executable and
file-based examples in the `shodo::font` module documentation.

CSS descriptor registration checks `max_font_descriptor_bytes` (64 KiB per
face) and `max_layer_descriptor_bytes` (8 MiB per layer) before validating
ranges, copying/indexing metadata, or trying sources. The logical allowance is
twice the family UTF-8 length plus 16 bytes per declared range: original family
and its lookup key, original range pairs and their maximum coverage-index /
sorting payload. Raw ranges are counted before any union normalization, so
duplicates do not bypass admission. Fixed headers and allocator overhead are
excluded; caller-owned spare capacity is compacted before storage. These are
logical payload budgets, not process RSS limits.

Registered CSS descriptors are immutable and shared internally. Coverage uses
a private sorted interval union and binary search; the public descriptor getter
still returns the original range order and duplicates. Empty ranges still
cover all scalars. Prepared-cache misses, disabled caches, and long-grapheme
coverage keys do not copy or hash the range payload. A source list prepares
the descriptor once and shares it across attempts. Equivalent alias coverage
shares memoized results; precomputed hashes are verified against interval values
so collisions cannot change coverage. Only a successful
registration consumes the layer allowance, checked again under the publication
lock; failures preserve faces and generation. Existing shared-layer descriptors
remain visible without being readmitted against a document's smaller cap.
`None` and `Limits::unlimited()` disable these limits for trusted input.
Complete `Limits` struct literals need the new fields; default-update literals
remain supported.

Source registration checks the entire declared list before attempting it:
`max_font_sources` defaults to 64, and `max_font_source_name_bytes` to 64 KiB of
total local-name UTF-8 bytes. A limit violation returns `FontError::Limit`
without registering an earlier source or falling through to later data.

Local names use all localized full/PostScript strings with Unicode string
lowercasing. A root-owned index shares names across clones and document layers;
different missing names neither reparse the
catalog nor load or touch its source blobs. Construction uses the root's
`max_local_font_index_bytes` (8 MiB of accounted metadata/construction storage)
and `max_local_font_lookup_work` (2^24 family/face/load/record/encoded-byte work
units). Index failure is retained: unknown names return the same limit error
without restarting, while already indexed names remain usable. These are
logical budgets, not process RSS limits; transient platform mappings are not
retained by the index. Selected path-backed names are verified against the
loaded bytes, using the caller's lookup-work cap per registration operation.

Installed names are a catalog snapshot, including files unavailable during
initial construction. Recreate the shared collection to discover replaced or
newly available installed files, or to raise exhausted index budgets. Root
`register` additions do not rebuild the native catalog. Once lookup has started,
new registered and materialized root faces must also fit the name-index budgets
before publication: registration returns a limit error without changing faces,
blob accounting, generation, or previously indexed names; matching skips a face
that cannot be admitted. Before the first lookup, name indexing remains lazy.
Document CSS faces do not become installed local sources. `None` disables each
budget, and `Limits::unlimited()` is available for trusted input.

`Limits::max_faces_per_layer` and `max_layer_blob_bytes` bound all retained
fonts in a layer, including explicit registrations, CSS `local()` sources, and
faces loaded lazily from the platform catalog. The shared built-in stub counts
as one face. Shared blob storage is charged once in that layer. Individual font
validation limits also apply to native files.

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
For physical float insets, use
`LineConstraint::from_physical_insets(container_inline_size, left_or_top, right_or_bottom, writing_mode, direction)`.
It subtracts both insets and sets the logical start offset, including RTL and
`sideways-lr` progression. Pass left/right insets for horizontal writing and
top/bottom insets otherwise, with the effective inline direction (LTR for
upright vertical text). Set `block_offset` and float cursors on the returned
constraint as needed. A constraint built with `LineConstraint::new` still takes
the available size after insets have been subtracted by the caller.

Ruby readings are retained child Lines; compose their transforms before physical
conversion and keep their source datasets separate. See the [ruby contract](ruby.md).

Read build warnings through `Paragraph::warnings()` and line layout warnings through `LayoutContext::take_warnings()`. Resource limit violations are returned as `LimitExceeded`.

`Limits::max_style_bytes` bounds logical owned style data and builder interning
keys to 64 MiB by default. Checks precede caller-data cloning and key allocation;
normal/first-line output and ruby input occurrences share their respective
budgets. The count covers style slots, all String/Vec payloads, and the fixed
fingerprint/index keys (12 bytes per interned pair). Keys borrow the retained
styles during lookup; no Debug strings or extra style payloads are retained.
It excludes allocator overhead, spare capacity, and bounded temporary
copies, so it is not an RSS limit. Exceeding it returns `LimitKind::StyleBytes`
and stops further builder input. `None` disables this budget, as does
`Limits::unlimited()` for trusted input. Existing `InlineStyle` field types and
font/paint behavior are preserved. Code that enumerates every field in a `Limits`
struct literal must add the new field; `..Limits::default()` remains supported.

`Limits::max_mapping_bytes` defaults to 64 MiB of admitted logical transform and
offset-mapping data per build. Checks precede new transform spans, indivisible
ranges, width-origin records/owned strings, and DOM/generated mapping records.
Each mapping record also reserves its private DOM/text/generated range and end
indexes, logical grouping entries, and construction scratch; public offset
queries can remain lazy and infallible. Coalesced records pay once, and a
one-for-one remap carries the original record's reservation.

Normal, first-line (including analysis reuse), and nested ruby datasets share
the admission budget. Imported bases also retain their own occurrence-local
caps, including generated isolate controls and remap splits. Producer records
retired during analysis remain charged; first-line reuse reserves the normal
pass's producer allowances before cloning. The count excludes allocator
overhead, spare capacity, fixed container headers, and bounded scalar scratch,
so it is not a process RSS limit. Public `OffsetMapping` clones retain the same
bounded dataset; caller-owned clones do not share a build's aggregate quota.
Exceeding the cap returns `LimitKind::MappingBytes` from analysis/build without
truncating text, dropping mapping, or merging expanded scalar boundaries.
Transform provenance is bounded even with `with_offset_mapping(false)`.
`None` and `Limits::unlimited()` disable this budget for trusted input.

## Shaping data and budgets

`Limits::max_analysis_bytes` defaults to 128 MiB of cumulative admission
allowances per build. Before each analysis phase, it reserves logical payload
and conservative variable scratch: raw whitespace flags, transform case/word
context and text-combine omissions, first-line source projection/cuts, grapheme
and typographic tables, caret merge capacity, break opportunities/profiles,
projection, bidi byte levels/paragraphs, and dependency scratch. Later TCY bidi
and per-scalar geometry scratch, and first-line cursor/cut tables, receive
allowances during analysis too. Reservations use processed UTF-8 bytes and
scalar counts; a long combining grapheme or authored controls cannot evade them.

Normal, first-line and nested ruby analysis share the cap. Imported bases keep
their occurrence-local and ancestor caps. A paint-only clone reserves the same
phase allowances as reference replay before copying; shared pre-transform
source cuts are charged once. Retired phase scratch is not refunded. Sequential
dependency operations within a phase share its scratch allowance; this does
not count total allocator traffic or bound CPU time. The ICU auto allowance is
audited against the baked 2.3.0 codepoint models (27 hidden units); the bidi
allowance against unicode-bidi 0.3.18. Dependency/model updates must re-audit
those assumptions.

This budget excludes allocator overhead/spare capacity, fixed container
headers, bounded stacks/model temporaries, and caller-owned independent
analyses. Existing text, style, mapping, item and glyph budgets still apply;
shaping output and per-glyph paint data use the glyph budget. It is not a
process RSS limit. Exceeding it returns `LimitKind::AnalysisBytes` from
`analyze`/`build` before the rejected phase starts, without truncating text,
discarding controls, or changing Unicode/caret boundaries. `None` and
`Limits::unlimited()` disable it for trusted input. An exhaustive `Limits`
literal must include `max_analysis_bytes`; struct-update syntax stays supported.

`Limits::max_items` also bounds the itemized shaping runs retained across normal,
first-line, and nested Ruby paragraph data. This prevents vertical orientation
and other shaping boundaries from creating unbounded `ShapeItem` allocations
before shaping limits are applied; exceeding the cap returns `LimitKind::Items`.

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

Main and first-line processed text, items, styles, mapping data, and glyphs share
each build budget. Default shaping runs are bounded by 64 KiB and paragraph glyph output by
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

Ruby line measurement is bounded the same way. Each `next_line` or
`intrinsic_sizes` call may measure ruby containers `max_ruby_line_work` times
(16 by default) per unit its fit probes cover plus per container of its widest
container walk, and measures a walk wider than 16,384 containers for one
look-ahead end per start only. Past that, the rest of the call fits lines (and measures intrinsic
sizes) without ruby annotation adjustments and warns once ("ruby line
measurement budget exceeded; fitting without ruby adjustments"); accepted lines
still place their ruby exactly, so only the break position (or the intrinsic
size) can differ. Ordinary text spends a few units per unit, far below the
allowance; the limit stops inputs that would re-measure every container at
every probe (quadratic work in one line). Whether a call exceeds it does not
depend on what earlier calls left in the context.

Each `LayoutContext` is `Send` and deliberately not `Sync`: use one per thread.
It reuses shaping scratch and at most 64 font-qualified shaping plans, and keeps
the reshaped line-edge windows of the most recent paragraph (at most 2048 entries
within a 1 MiB accounted storage budget). Accounting includes vector capacities,
retained run instances and a metadata allowance; it does not measure allocator
overhead or process RSS. A single window costing more than 1K glyph-equivalents
is not retained. When full, the cache keeps resident results and declines new
ones until the paragraph changes or the cache is cleared; no word cache is retained.
`shrink_to(bytes)` drops all plans and that window cache and caps the combined
accounted scratch/partial-line buffers, preferring scratch when it fits. It does
not measure separately shared paragraph/font allocations. Shaping also releases
scratch exceeding the current run budget's conservative storage bound. See the
[measurement record](../records/shaping-measurements.md) and
[historical harfrust status proposal](../records/harfrust-shaping-status-proposal.md).

Shodo uses HarfRust 0.15. `FontCollection::shaper_data(id)` returns an
`Arc<harfrust::Font>` containing shared font bytes and layout caches. This replaces
the earlier `Arc<harfrust::ShaperData>` return type. For direct shaping, construct
`harfrust::ShaperFont::new(&font)` and pass a mutable `harfrust::Buffer` to
`harfrust::shape`. Select variations with `font.instance_builder()` before
constructing the shaping font. Retained handles remain usable after eviction or
collection drop; the font bytes are shared rather than copied.

## Horizontal output and hit testing

`GlyphRunView::source()` returns the caller source of the run's first scalar,
including its DOM byte offset after collapsing and transformation. Every
`Fragment::GlyphRun` contains at least one glyph, but a shared cluster can cover
several source nodes with one paint owner. For pagination, iterate `Line::owners()`
once per line to index all contributing DOM text nodes and their original byte
ranges. Adjacent or overlapping ranges of one node coalesce; disjoint ranges
remain separate. Collapsed gaps at a wrap belong to the preceding line, and
leading collapsed bytes to the first line. Each accepted line uses its own
mapping dataset, including `::first-line` transformations. Generated content and
object anchors have no DOM ranges. Ruby child lines expose their own owners.
These source queries require offset mapping (enabled by default); if disabled,
`source()` returns `None` and `owners()` is empty, while `node()` still works.

`GlyphRunView::physical_origin(index, container_size)` returns a glyph outline's
physical `(x, y)` in the line's layout container, including `Line::block_offset()`.
Pass the full physical container size: RTL origins are measured from its inline
end, and vertical-rl block positions from its right edge. It uses the effective
inline direction and natural shaping advance, so letter spacing and justification
do not shift RTL outlines within their cells. `glyph_origin()` remains line-local
and logical. Apply `glyph_transform()` and `PhysicalConverter::vector()` to
outline vectors. Retained ruby child lines need their annotation transform
composed with the logical origin; then add the parent line's block offset and
convert with the parent line's physical converter.

### Standalone text and draw origins

For a standalone painter following this convention, `(draw_x, draw_y)` is the
physical top-left of the layout container. Convert each glyph's origin relative
to that container, then apply the painter's translation. For horizontal RTL,
inline-start is at the container's right edge.

Construct the converter from each accepted line's `writing_mode()` and
`used_direction()`. Use the full physical layout container, including the inline
constraint used for alignment. The content's ink width or `Line::inline_size()`
does not substitute for that constraint. Upright vertical text can use LTR even
when its inherited CSS direction is RTL.

For an ordinary glyph run and a caller-owned logical alignment correction:

```rust,ignore
let converter = PhysicalConverter::new(
    line.writing_mode(), line.used_direction(), container_size,
);
let (inline, block) = run.glyph_origin(glyph_index).unwrap();
let local = converter.point(
    inline + hang_shift, block + line.block_offset(),
);
let origin = (draw_x + local.0, draw_y + local.1);
```

Here `hang_shift` moves content along the logical inline axis. For horizontal
flow, with container width `W`, glyph origin `(i, b)` and shift `s`:

| Used direction | Physical glyph origin |
| --- | --- |
| LTR | `(draw_x + i + s, draw_y + b + line.block_offset())` |
| RTL | `(draw_x + W - (i + s), draw_y + b + line.block_offset())` |

A positive shift therefore moves RTL content left. For example, with `W = 100`,
`draw_x = 20`, `i = 10` and `s = 5`, the physical x is 35 for LTR and 105 for RTL.
With no correction, `run.physical_origin(glyph_index, container_size)` performs
the conversion and includes `block_offset()`; add only the painter's physical
translation to its result. `glyph_origin()` already includes the natural shaping
advance needed by RTL: do not add an advance or subtract tracking again.

Convert outline vectors separately with `run.glyph_transform()` and
`converter.vector()`, as in the [vertical output guide](vertical-layout.md).
This preserves the glyph's physical orientation while positions follow the
inline direction. Do not mirror font outlines to implement RTL. Retained ruby
child lines follow the annotation-transform composition described above.

In raikiri [commit `ca5e5477`](https://github.com/fulgur-rs/raikiri/commit/ca5e547700951fa3b8b757d4ca04904424bcc3ef),
`StandaloneText::container()` retains the constrained layout extent and
`standalone_text::draw` follows this conversion with `hang_shift(index)`.
These helpers belong to that upstream implementation; the shodo development
dependency is pinned to the earlier `a62ea75b`, which predates the standalone
helpers. With no inline constraint, raikiri uses its measured line width excluding
trailing blanks. Its
`rtl_start_and_end_align_to_opposite_physical_edges_without_mirroring` and
`hebrew_run_stays_at_the_rtl_start_edge_without_mirroring` tests cover alignment
and unchanged outline orientation. RTL standalone origins are supported by that
version.

`GlyphRunView::metrics()` uses the same face, effective size and variation
coordinates as shaping. `Line::metrics()` separates the final line-box extents
from the root font's text-over/text-under edges. Every participating run can
contribute to line height; block padding/borders remain paint geometry.
`InlineBoxFragment::rect` and `content_rect` exclude collapsible line-end spaces
from background and border widths. Preserved spaces remain inside their boxes,
including when they hang. Text source ranges and glyph advances still retain the
space for selection; do not subtract `Line::hang_end()` from these box widths.
`Line::overflow_rect()` returns nominal glyph ink and painted box bounds in
line-local coordinates. Add `block_offset` before physical conversion. Renderer
strokes, antialiasing and decoration effects can extend those nominal bounds.
`AnchorFragment::block_offset` already includes the accepted line's offset in
the layout container. Place an out-of-flow child's static anchor by converting
`(inline_position, block_offset)` with the line's writing mode and used
direction; do not add `Line::block_offset()` again. Retained anchors carry the
caller's final placement even when a line was retried or placed after a gap.
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
borrows them. Coordinate search trees are prepared on the first coordinate hit;
caret lookup, navigation and selection use the source index directly. Include
that first-hit preparation when measuring pointer interaction, as well as index
construction when measuring source-only use. Each `TextPosition` identifies a
line, a UTF-8 offset in that
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
See the executable [horizontal hit example](../../crates/shodo/examples/horizontal_hit.rs) and the
[contract coverage](horizontal-layout-contracts.md):

```sh
cargo run --example horizontal_hit -- path/to/a-font.ttf
# Without a font path, the example uses deterministic missing-font fallback.
cargo run --example horizontal_hit --no-default-features
```
