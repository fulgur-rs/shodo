# Ruby input and retained output

shodo lays out ruby bases in the parent's inline text stream and shapes readings
as separate annotation paragraphs. Pairing, coordinated breaks, alignment,
overhang, line spacing and horizontal/vertical placement belong to shodo. The
caller resolves CSS, normalizes HTML anonymous boxes, supplies styles and renders
the accepted output. No raikiri integration or HTML parser is required.

## Constructing ruby

`RubyContent::text(source, text, style, limits)` captures one styled span.
`RubyContent::from_builder(builder)` consumes an unshaped `ParagraphBuilder`,
retaining nested inlines, source offsets, first-line styles, limits, warnings and
any terminal build error. Clones share immutable input. A content snapshot can
contain another ruby container.

Supply `RubyBase { node, content, align }` entries and `RubyLevel { annotations,
style }` entries to `Ruby::new`. Each `RubyAnnotation` has `node`, `content`,
`span` and `visibility`. Append the result with
`ParagraphBuilder::push_ruby(container_node, container_style, ruby)` or
`RichText::push_ruby(ruby, container_style)`. The explicit first-line variant is
`push_ruby_with_first_line(node, normal, first_line, ruby)`; content builders can
also retain their own first-line styles. The caller determines which CSS styles
apply to each of these inputs.

For example, after resolving `base_style` and `reading_style` and choosing
caller-owned node IDs:

```rust,ignore
let ruby = Ruby::new(
    vec![RubyBase {
        node: NodeId(10),
        content: RubyContent::text(
            TextSource::Dom { node: NodeId(10), offset: 40 },
            "日本語", &base_style, &limits,
        ),
        align: RubyAlign::SpaceAround,
    }],
    vec![RubyLevel {
        annotations: vec![RubyAnnotation {
            node: NodeId(20),
            content: RubyContent::text(
                TextSource::Dom { node: NodeId(20), offset: 70 },
                "にほんご", &reading_style, &limits,
            ),
            span: RubySpan::All,
            visibility: RubyVisibility::Visible,
        }],
        style: RubyStyle::default(),
    }],
)?;
builder.push_ruby(NodeId(8), &base_style, ruby);
```

The [complete fixed-font example](../dev/fixtures/examples/ruby_png.rs) registers
real CJK font bytes, builds paragraphs and paints retained output after the input
paragraph and layout context have been dropped.

## Pairing, visibility and breaks

`RubySpan::Auto` pairs annotations with columns in order. Missing counterparts
become anonymous empty boxes, with no fabricated source node. `All` spans the
whole segment. `Columns(start..end)` specifies a nonempty range in the normalized
column table. `Ruby::new` rejects out-of-range, empty or overlapping explicit
spans in one level with `RubyError`. It validates structure without shaping.

`RubyMerge::Separate` is the default. `Auto` currently uses Separate. `Merge`
groups each line's accepted annotation fragments as a spanning reading while
preserving their individual source datasets and original pairings across wraps.
Separate/Auto automatically hide a reading whose original text content equals
its associated base, before whitespace processing or text transforms. Merge
disables this automatic hiding.

`Visible` paints the retained lane. `Hidden` reserves its geometry and spacing
but suppresses its complete paint/hit subtree. `Collapse` removes annotation
sizing and paint while preserving pairing. Hidden lanes remain explicit output
metadata, with a visibility flag; collapsed lanes have no retained paint view.
Annotation-only forced breaks are processed as collapsible segment breaks.

Within a pair, a cut requires a legal source-safe opportunity in the base and
every active lane. shodo coordinates nearest proportional typographic progress,
choosing the earlier cut on a tie. It never splits UTF-8, graphemes, indivisible
transform expansions, shared shaping clusters or text-combine compositions. A
pair with no compatible cut overflows whole. Every accepted continuation consumes
its base and reading ranges once; first-line alternates map continuations back
to normal source identity. A retry from the same `BreakToken` has no mutable
annotation cursor to advance accidentally.

Ruby widths participate in ordinary, intrinsic, balance/pretty and float retry
measurement. The same selected lane ranges and real shaping windows determine
fit and accepted placement.

## Geometry and spacing

Base alignment uses `RubyBase::align`; reading alignment uses `RubyStyle::align`.
Both default to `SpaceAround`. `Start`, `Center`, `SpaceBetween` and `SpaceAround`
distribute excess width over actual typographic units. A single SpaceBetween
unit centers. Glyph scale and `Cluster::shaping_advance` stay unchanged; assigned
layout advances carry spacing. Source carets and selections follow the aligned
glyphs rather than the column's empty padding.

`RubyPosition::Over` and `Under` are line-relative sides. `Alternate` and
`AlternateUnder` alternate levels starting on the named side; levels on one
side stack outwards. Alternate is the default. InterCharacter makes an upright
vertical reading lane beside horizontal bases and behaves as Over in vertical
containers. Over/Under in VerticalRl and VerticalLr preserve the same physical
line-over/line-under meaning. Explicit annotation font sizes are never replaced
with a fixed extent or implicitly scaled.

Default `RubyOverhang::Auto` admits extension only over actual eligible same-line
plain neighbors, limited by their geometric clearance and half the reading's
full-width-character advance. Atomics, other annotations and line edges block
overhang. `None` fully reserves the chosen column width.

Font-content extents determine placement; annotation line-height does not inflate
the annotation content box. Container leading is reused before extra leading is
added. For one baseline-aligned base with content height B, line-height H and
over/under stacks O/U, untrimmed line advance is `max(H, B + O + U)`. Excess
leading goes to the occupied side or proportionally to both. Nested ruby,
vertical alignment and text-box trimming preserve annotation clearance.
`max_block_size` applies to the resulting advance.

`Line::overflow_rect()` includes visible annotation ink and can extend beyond
the advance box. It remains line-local; strokes, antialiasing and decorations
may extend it further. The renderer decides canvas size and clipping policy.

## Painting and source positions

Bases remain ordinary `Fragment::GlyphRun` output. `Fragment::RubyAnnotation`
and `Line::ruby_annotations()` expose `RubyAnnotationView`: container/base/
annotation nodes, level, base and reading text ranges, visibility, retained
`Line`, its `Paragraph`, origin and full `RubyTransform`.

`Line::fragments()` yields non-ruby fragments in their visual order first, then
retained annotations in the same order as `Line::ruby_annotations()`.
`Line::fragment(index)` indexes this sequence, which does not guarantee a global
physical visual order. Use `RubyTransform` for bidi and vertical placement.
Nested annotations remain on the reading's child `Line`; traverse that line and
compose their transforms.

Annotation text never enters the parent's `Paragraph::text()` or primary
`OffsetMapping`. Each processed dataset can contain nonpainting bidi controls;
text ranges are UTF-8 offsets in their own accepted Line dataset. DOM offsets
retain the original nodes and caller offsets, including transformed content.

Render each accepted glyph once with its actual font bytes, face index, size,
variation coordinates and public glyph transform. Reading geometry is local to
its child Line. Compose its RubyTransform with any outer transforms, then add
the root line's block offset and convert through the root `PhysicalConverter`.
Do not add the view's origin again: it is already the transform's translation.
Nested lanes repeat this composition, without reshaping strings or inferring
fonts from node IDs. Read local `paint_spans()` for source decorations; those
rectangles already include their own Line block offset.

Main `LineLayout` navigation and selection retain base reading order and sources.
Ordinary `hit_test` on a visible annotation routes to a caret in its paired
base. `hit_test_ruby` returns `RubyHit` with annotation-local position/source,
`parent_line()` in the supplied root Lines, and an outer-to-inner `path()` of
retained views. Use a child `LineLayout` over `annotation.line()` to select or
navigate a reading; its offsets never belong to the main Line.

`AccessibleLayout::ruby_annotations()` yields `AccessibleRuby { parent_line,
annotation }`. Construct a child AccessibleLayout for reading-local characters
and source anchors. Main logical text and character positions include bases
without silently inserting annotation characters.

With AccessKit enabled, paired main TextRuns link through `details` to separately
identified `Role::RubyAnnotation` nodes. Their value carries accepted reading
content and their children own nested readings; Hidden status propagates. These
metadata nodes are not main TextRuns and have no main text-position entries.
Base character/byte offsets and selection stay unchanged, including when a base
and reading deliberately share a DOM anchor. The adapter reuses source-based
IDs; the caller still owns ID allocation and platform delivery. An editor can
export a reading's child AccessibleLayout separately with its own adapter/root.

## Limits and browser comparison policies

All retained text, items, styles, shaped glyphs and first-line alternatives count
against the parent's aggregate Limits, including nested or reused snapshots.
Each imported base also keeps its snapshot limits for retained text, items,
styles and shaped glyphs: its isolation wrapper,
normal and first-line resources, and nested readings share that occurrence's
cap. Its shaping-run byte cap also bounds each construction run.
Reused snapshots keep separate occurrence scopes. Cells shared with
unrelated containers remain in the parent budget; index cells exclusively
owned by a base count against its own items cap.
Nesting and projected anonymous-box allocation are checked before excess work.
Snapshot terminal errors survive import. Original font assets and non-ruby
snapshot expectations are unchanged.

The [CSS Ruby Level 1 draft](https://drafts.csswg.org/css-ruby-1/) describes HTML
anonymous boxes, grouping choices and permitted edge effects. This API receives
normalized structure rather than performing browser HTML/CSS processing. Its
Auto merge policy is explicitly Separate; overhang uses the conservative
clearance rule above; coordinated interior cuts use a deterministic proportional
policy. These choices require a dedicated browser measurement contract before
claiming parity with a particular browser/version. Existing browser recorder
fixtures exclude ruby and are not evidence of ruby interoperability.

Run the real-font example with:

```sh
cargo run -p shodo-fixtures --example ruby_png -- target/ruby-png
```

It writes horizontal, VerticalRl and VerticalLr PNG/JSON pairs. Each records
independent base/annotation source ranges, actual glyph IDs, font checksums,
sizes, advances, origins, transforms and overflow. Fixed-font tests cover
coordinated continuation, actual source anchors, hit/selection/accessibility,
asymmetric vertical pixels, nested displacement, intrinsic emoji color and
strict clipping. The shared example painter supports outlines and opt-in
CBDT/CBLC PNG; other color formats and CSS decoration propagation remain
caller/backend responsibilities.
