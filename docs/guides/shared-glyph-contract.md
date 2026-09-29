# Shared glyph paint and source-region contract

A shaping cluster can cross caller text nodes. Three nodes containing `f`,
`f`, `i` can produce one `ffi` glyph. The accepted `Line::fragments()` output
contains that glyph once. Paint each `GlyphRunView::glyphs()` once using its
actual font data, glyph ID, size, coordinates and synthesis, without reshaping
strings or drawing a second copy for another source node.

## Paint ownership

`GlyphRunView::node()` is the source item's NodeId, normally the text node
passed to `TextSource::Dom`, not the enclosing `open_inline` element's ID.
The item supplying a shared cluster's first scalar owns that cluster. Its
run's `text_range()` can cover subsequent source nodes as well. The owner
is a style lookup key, not proof that it owns all source text in that range.
`style_index()` identifies layout input; it does not provide CSS color or
link metadata. The caller resolves the text node's appropriate paint style,
including its element ancestry and the accepted first-line style if relevant.

The demonstrated policy paints the entire shared outline in its owner's
color. Identical colors and differing colors both preserve shaping and draw
one outline. This is a deliberate rendering policy, not a guarantee of browser
pixel equality. CSS Text [§1.4](https://www.w3.org/TR/css-text-3/#characters)
allows a typographic unit divided by an element boundary to belong to either
side, or an approximation of both. [§7.3](https://www.w3.org/TR/css-text-3/#boundary-shaping)
requires shaping to continue where only nonglyph formatting changes, including
text decoration, occur. Changing color, adding a link or changing underline
must therefore not be implemented by silently splitting shaping or duplicating
glyphs. Actual shaping barriers (spacing edges, vertical alignment, bidi
isolation, font changes) retain the existing core rules.

## Source identity and interaction

Enable `ParagraphBuilder::with_offset_mapping(true)`. For each accepted line,
use **that line's** `text()`, `text_range()` and `offset_mapping()`; first-line
transforms and following lines can use different processed datasets. Keep the
original text NodeId/UTF-8 ranges and ancestor link/decoration metadata in the
caller. Several text nodes can belong to the same link element.

1. Intersect the node's `OffsetMapping::units()` processed ranges with the
   accepted line's range. They can be discontiguous; never treat an ancestor
   element's IDs or a glyph owner's whole run as the node's source interval.
2. Query `LineLayout::selection_rects` using downstream affinity at the range
   start and upstream at the end. These are logical rectangles, with line
   block offsets already included. Resolve writing-mode/viewport transforms
   in the caller. Keep bidi gaps rather than enclosing unrelated text.
3. Associate these regions with the source node's link element and source
   decoration metadata. The example draws a simple underline under only the
   middle `f` interval, independently of the red `ffi` glyph's first owner.
   It is a connection example, not CSS decoration propagation, skip-ink,
   underline positioning or thickness conformance.
4. Resolve pointer link identity by region containment in caller coordinates.
   `hit_test` instead returns the nearest caret stop and its source origin;
   it does not return an enclosing link element. Boundary affinity decides
   which adjacent source owns a stop. Use `caret` and navigation for text
   editing, preserving the selected stop's affinity.

Inside `ffi`, supported GDEF caret positions, or proportional grapheme stops
when absent, give three positive source intervals that partition its advance.
These are interaction advances, not exact glyph ink outlines. In general,
indivisible graphemes/transforms snap according to affinity: source boundaries
inside one indivisible unit do not promise distinct positive rectangles.
Collapsed whitespace may have no painting region. Generated content has a
`TextOrigin::Generated` instead of a DOM offset. Handle these explicitly rather
than inventing source offsets or duplicating a shared outline.

RTL fragment subdivision can change enumeration order when source boundaries
change. The positioned glyph coordinates and logical source clusters are
canonical for comparison; sorting by processed cluster for evidence must not
change the painter's actual positioned output.

## Executable evidence and reuse

`dev/fixtures/tests/shared_glyph.rs` uses checked-in Latin and Arabic font
bytes, with system font discovery disabled, to check:

- three-source `ffi`, single owner/draw, equal and different colors;
- all three source mappings, positive partial selections, carets and hit tests;
- a middle-source link and simple underline, independently of outline ownership;
- Arabic cross-node joining, LTR/RTL source regions and narrow-line reshaping;
- actual pinned raikiri parse/cascade -> shodo -> accepted-glyph outline paint.

The real cascade case uses three inline elements, the middle an underlined blue
link. Each source text ID is retained; its enclosing element resolves paint
color and link metadata. Shodo emits one red `ffi` glyph and the caller emits
only the middle link's blue annotation. The tests verify glyph counts, source
regions, actual glyph coordinates and pixels. They do not claim full CSS,
raikiri renderer replacement, WPT or browser parity.

```sh
SHODO_SHARED_GLYPH_PNG=/tmp/shared-glyph.png \
  cargo +stable test --offline -p shodo-raikiri --test shared_glyph
```

The fixed-font [glyph painter](../../dev/harness/src/glyph_paint.rs)
is shared by rendering examples and snapshot tests. These contract tests use
unsynthesized outline fixtures and a simple source annotation. Production
synthesis, color fonts, full decorations and general page painting remain caller
responsibilities.

## Renderer integration requirements

The integration tests establish that the existing public glyph, mapping,
selection/caret and hit-test APIs supply the required data. **No new public
API is required for this contract.** Renderers should use one draw per glyph run,
source-text-to-style resolution and separate source-region annotations. Link
and source decoration integration are required at the production handoff;
whole-owner-run link areas or painting each source's entire shared cluster
would lose or duplicate information.

These tests establish the shared-glyph handoff, not a complete raikiri renderer
integration. Production callers still need to resolve source styles and link
metadata, apply CSS decoration policy, and paint the rest of the page. The
outline example alone does not establish full browser rendering equivalence;
see the separate [paint contract](paint-styles.md),
[snapshot tests](../dev/snapshot-tests.md), and [browser comparison](../dev/browser-comparison.md)
for their supported behavior and validation scope.
