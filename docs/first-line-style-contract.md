# Resolved inline first-line style contract

`ParagraphBuilder::open_inline_with_first_line(node, normal, first_line, edges)`
and `RichText::push_with_first_line(text, normal, first_line)` accept a resolved
normal style and a resolved first-line style for each inline/span. With a16px
normal root and a32px first-line root, an explicitly declared16px child can
supply16px for both variants, while an inheriting child supplies16/32px.
Shodo does not compare the explicit alternative with the root to infer CSS
inheritance, declarations or relative units. Resolve them in the caller.

```rust
use shodo::{LayoutContext, ParagraphBuilder};
use shodo::font::FontCollection;
use shodo::limits::{LimitExceeded, Limits};
use shodo::node::{NodeId, TextSource};
use shodo::style::{InlineStyle, ParagraphStyle};

fn build(fonts: &FontCollection, cx: &mut LayoutContext)
    -> Result<shodo::Paragraph, LimitExceeded>
{
    let normal = InlineStyle::default(); //16px; configure the application's fonts
    let first = InlineStyle { font_size:32.0, ..normal.clone() };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle { root:normal.clone(), first_line:Some(first.clone()),
                          ..Default::default() },
        &Limits::default(),
    );
    b.open_inline_with_first_line(NodeId(1), &normal, &first, Default::default())
        .push_text(TextSource::Dom {node:NodeId(2),offset:0}, "inherited ")
        .close_inline()
        .open_inline_with_first_line(NodeId(3), &normal, &normal, Default::default())
        .push_text(TextSource::Dom {node:NodeId(4),offset:0}, "explicit16px")
        .close_inline();
    b.build(cx, fonts)
}
```

An explicit alternative activates the existing first-line path even when
`ParagraphStyle::first_line` is absent; in that case the root strut retains
its normal style. For direct root text, set the root alternative through
`ParagraphStyle::first_line`. Edges have one caller-owned geometry for both
variants. Atomic styles retain their existing input contract; parent inline
style pairs also carry parent metrics for their subtree.

The supported alternate property set is unchanged: font families, size,
weight, width, style, variations, features, kerning, variants, optical sizing,
synthesis and size adjustment; language; line height; letter/word spacing;
text transform and emphasis. The remaining `InlineStyle` formatting properties
retain the normal values. These inputs do not add support for other CSS
first-line properties or perform cascade. Line options remain caller-owned.
Color, backgrounds and decorations are not `InlineStyle` fields: the painter
uses the appropriate caller-resolved style table for the accepted first line
and normal lines, together with each glyph run's original owner.

Normal/alternative pairs are interned together: equal normal styles with
unequal alternatives remain distinct, while equal pairs share an index.
This must not insert a normal shaping barrier; the fixed-font test keeps a
cross-node ffi ligature intact on the normal continuation. Explicit alternate
styles have sparse storage, so legacy inputs allocate no alternate style table
in the builder. Both copies count against the build's style budget; an input
limit violation occurs before copying another explicit pair. Normal and
alternate processed text/items/glyphs continue to share their existing limits.

Only the first accepted formatted line uses the alternative. Repeating a
trial with the same `BreakToken`, including float retry, does not consume it.
The accepted token continues into normal styles, even where uppercase expands
ß into SS. Forced/block boundaries retain their existing behavior. Use each
`Line::text()` and `Line::offset_mapping()` with that line's ranges; a normal
continuation may start with a generated float anchor before its first text run.

## Compatibility and migration

Existing `open_inline`, `RichText::push` and `ParagraphStyle::first_line`
inputs retain their previous behavior: descendant properties equal to the
normal root inherit the root alternative, while differing values are kept.
That fallback is deliberately unchanged for existing users. Mixing old and
new inputs leaves that fallback on the old inputs, including nested ones.
Migrate every inline that needs exact resolution, supply its complete resolved
normal/alternate pair, and retain the paragraph's resolved root alternative
when root text or a different first-line strut is needed. Shodo does not infer
nested inheritance from the explicit parent's alternative.

## Real cascade and paint evidence

`dev/fixtures/tests/first_line_cascade.rs` uses raikiri-html/style public APIs
at commit `ab7e619a8f321f03de8b8c8b9342954868e044c8`. It parses one actual DOM,
retains its normal cascade, appends a root rule with `Origin::Author` to the
original rule tree and obtains a second real cascade over that same DOM.
Both tables supply each inline's values to the new API. An author-origin
root override is a controlled producer probe, not a CSS pseudo-element
implementation or a substitute for an actual first-line cascade provider.

| Child fixture | Normal font size | Alternate font size |
| --- | ---: | ---: |
| unspecified/inherited |16px |32px |
| explicitly equal to normal root |16px |16px |
| different explicit size |20px |20px |
| nested150% under explicit20px parent |30px |30px |
|150% of the resolved root |24px |48px |

The test checks actual run fonts/sizes, source-node mapping, line metrics and
outline rasterization of five accepted glyphs. It uses checked-in Latin font
bytes and colors from the alternative cascade, including the explicitly blue
child among red inherited children. It does not reshape text for painting.
Core fixture tests additionally verify exact-equal children, a nested resolved
alternative, forced-break and float-retry normal continuation, rich text
activation without a root override, pair budgets and normal shared ligatures.

Generate the inspected PNG with:

```sh
SHODO_FIRST_LINE_PNG=/tmp/resolved-first-line.png \
  cargo +stable test --offline -p shodo-fixtures --test first_line_cascade
```

The pinned raikiri parser/cascade does **not** expose `::first-line`: adding
`#root::first-line{font-size:32px}` leaves the root16px and the pseudo map empty.
A separate regression test records that producer limitation. Production CSS
first-line integration therefore still needs a producer that resolves per
inline alternatives; this change removes shodo's ambiguous input contract,
without implementing that missing producer. The preservation/switch necessity
assessment remains in the unmerged S4 investigation (`shodo-p2m.5`), and this
normal API change does not merge or enable that spike.

CSS inheritance and excluded properties are specified by
[CSS Pseudo-Elements4 §2.1.3](https://www.w3.org/TR/css-pseudo-4/#first-line-inheritance).
The test's two ordinary author cascades establish the handoff boundary; they
are not evidence of full CSS first-line conformance or WPT PASS.
