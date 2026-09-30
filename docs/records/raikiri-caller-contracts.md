# Representative raikiri caller contracts

> Historical supplied-input record. `shodo-7ff` replaced the ordinary CSS
> supplier with actual `::first-line` resolution and updated all dependency
> pins. See [the real CSS measurement](raikiri-first-line.md) for the current
> API, supported scope and fixed WPT evidence. The old SHA below identifies
> this record's original measurement.

`dev/fixtures/examples/raikiri_contracts.rs` combines explicit first-line
inputs, accepted-line offset mappings, source links, and retained glyph
paint in one executable caller. Its support module is shared by the CLI
and its tests. It uses the pinned raikiri HTML/style/DOM crates already
used by the fixtures; it does not copy raikiri's implementation.

Run from the repository root:

```sh
cargo run -p shodo-raikiri --example raikiri_contracts -- /tmp/raikiri-contracts
cargo test -p shodo-raikiri --example raikiri_contracts
```

The CLI writes `caller.png` and `caller.json`. The fixed sample produces
two accepted lines, two source link regions, and two painted glyphs.
The first line's three DOM text nodes form one 32px red `ffi` glyph;
only the middle `f` links to `/target` and has a lime underline. The
second line retains its normal 16px style and links to `/later`.

## Resolved inputs and DOM identity

`resolve_fixture` parses the real HTML once and resolves two cascades on
that same DOM. The normal cascade comes from the document stylesheet;
the alternate cascade adds an ordinary root CSS override. Every inline
element supplies both its normal and alternate resolved style through
`ParagraphBuilder::open_inline_with_first_line`. The paragraph also
supplies its explicit first-line root style. Text nodes use
`TextSource::Dom` with their original IDs and byte offset zero.

An explicitly specified 16px child remains 16px when the alternate root
is 32px, while an inherited child becomes 32px. Comparing computed
values to decide whether a child inherits would lose this distinction.
The test also changes `ß` to `SS` on the first line, then checks that
the second accepted line uses the normal text and identity mapping.

The caller uses the builder API because it retains DOM text-node IDs.
`RichText::push_with_first_line` is the alternative for callers using
RichText's synthetic text IDs; using both is not necessary here.

**The fixture supplier is not a CSS `::first-line` implementation.**
At pin `ab7e619a8f321f03de8b8c8b9342954868e044c8`, raikiri-style's
`PseudoElem` and selector parser support `before`, `after`, and `marker`,
and reject `first-line`. A production supplier must resolve actual
pseudo-element rules into suitable normal/first-line inputs on the same
DOM. Follow-up `shodo-7ff` tracks that work after the S4 adoption decision.
This example proves the supplied-input consumer contract and does not
change either S4 spike or claim additional WPT passes.

## Source regions and paint

The paragraph enables `with_offset_mapping(true)`. Each accepted line's
own `OffsetMapping::units()` is intersected with that line's text range.
Identity units crop their DOM byte range by the same amount; expanded
units retain their contributing DOM range. Collapsed units have no
click region. This matters when one node spans multiple lines or when
first-line transformation selects a different mapping dataset.

The DOM walk associates each original text-node ID with its ancestor
link element and `href`. `LineLayout::selection_rects` supplies geometry
for the mapped text range, rather than the shared glyph owner's entire
range. Link regions retain text-node ID, link-element ID, DOM and text
byte ranges, mapping kind, and accepted-line index. Nested link children
can share a link while retaining different source IDs.

`Output::link_at` consumes container-relative logical coordinates.
The PNG adds a 10px canvas margin, so subtract that margin before using
PNG coordinates for hit queries. Rectangles use half-open bounds. JSON
records each region's rectangle and the link found at its midpoint.

Painting uses the existing retained-style fixture renderer and accepted
glyphs, including source paint spans. An ancestor's underline is carried
to its inline text descendants. The shared `ffi` glyph is drawn once;
its middle-source underline is limited to that source's caret interval.
This does not recolor the entire glyph for every contributing source.

## Verification and limits

Eight tests exercise the real pinned parser/cascade, shodo layout,
source mapping, logical hit regions, and raster output. They cover the
shared glyph's middle link, first-line font/color, explicit versus
inherited child styles, first-line transformation and later-line
mapping, per-line DOM clipping, collapsed link whitespace, nested link
sources with propagated underline, and alternate display rejection.

The geometry and underline pixel assertions use independent fixed-font
table values: Latin UPEM 1000, `ffi` glyph 367 with GDEF carets 315 and
631, hhea ascent 1069, and post underline position -100. At 32px, the
middle source starts at 10.08px and spans 10.112px. Layout metric
rounding is bounded by 1/64px. With the canvas margin, the 2px lime
underline contains pixel (24, 47) and excludes (12, 47) and (33, 47).

This is a development caller for the fixed examples, not a general CSS
adapter. It maps a block root with span/a/em/br descendants, horizontal
LTR text, the static normal 400-weight Latin fixture, absolute spacing,
computed line height, normal/pre legacy whitespace with supported
collapse/preserve and wrapping longhands, supported case transforms,
and a single solid underline with font offset. Other fields of the
raikiri computed style are not generally projected. Display checks require
a block IFC root and inline span/a/em descendants in both the normal and
alternate cascades; other display values for these elements are rejected.
The caller handles br as an unconditional forced break before inspecting
display in either cascade, so even `br { display: none }` still produces
a break in this fixed example caller. Font fallback, font axes,
vertical/RTL layout, full decoration layering, block descendants, ch or
percentage/calc used-value resolution, and general CSS layout require
separate integration work. Mixed noninitial legacy whitespace and
longhand values need declaration provenance and are rejected.

The JSON and tests are reproducible caller evidence; adopting these
contracts in the production integration remains part of S4/p2m.6.
