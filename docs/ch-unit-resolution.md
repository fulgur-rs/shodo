# Selected-font `ch` resolution

`shodo-3fl` investigated the `ch` rejections in the unmerged raikiri
integration experiment. For the horizontal cases reproduced here, shodo
already supplies the required measurement API. The missing step is in the
caller: resolve font-relative values after registering fonts, then pass
absolute lengths to shodo. This investigation does not replace the
production raikiri caller or merge either integration spike.

## Reproduce

```sh
cargo run -p shodo-fixtures --example ch_units
cargo test -p shodo-fixtures --example ch_units
```

The example uses the existing development-only raikiri HTML/cascade
revision `ab7e619a8f321f03de8b8c8b9342954868e044c8` and the checked-in
Latin/CJK font fixtures. System font discovery is disabled. It parses a
fixed parent block with one inline child, registers the fixture faces,
measures the preserved declaring-font keys, and builds an actual shodo
paragraph. Its JSON reports accepted text, line width, glyph font and
position. It is a fixed horizontal LTR reproduction, not a general CSS
adapter. RTL and non-horizontal CSSOM writing modes are rejected rather
than interpreted using the example's physical-to-logical edge mapping.

## Measurement and inheritance

`FontCollection::resolve_ch(&FontQuery, size)` selects the face supplying
U+0030 and returns its advance together with its `FontId`. Missing U+0030
uses the documented 0.5em fallback. See `src/font/metrics.rs`.
The unit is the advance of the zero glyph, not the width of arbitrary
paragraph text; [CSS Values and Units](https://www.w3.org/TR/css-values-4/#ch)
defines the font-relative unit.

The pinned raikiri cascade retains `ChFontKey` with family, size, weight
and style. Inherited spacing retains the key of the declaring element,
even when the child selects a different font. Margin/padding provenance
belongs to the element declaring the edge. The native raikiri path already
has `measure_ch_advance_for_font_key` in
`raikiri-dom/src/layout/inline_text.rs`; replacing that caller must preserve
this existing capability.

The independent fixture font-table values are:

| Face | UPEM | U+0030 glyph | hmtx advance | Size | Unrounded `1ch` |
| --- | --- | --- | --- | --- | --- |
| Shodo Fixture Latin | 1000 | 19 | 572 | 20px | 11.44px |
| Shodo Fixture CJK | 1000 | 17 | 555 | 40px | 22.2px |

With the parent Latin20px and child CJK40px:

| Property | Declaring key | Value | Expected unrounded length |
| --- | --- | --- | --- |
| inherited word-spacing | parent | 2ch | 22.88px |
| parent block text-indent | parent | 3ch | 34.32px |
| child inline margin, each side | child | 4ch | 88.8px |
| child inline padding, each side | child | 5ch | 111px |

The regression tests compare accepted-line geometry against these literal
font-table expectations, with less than 1/64px tolerance for layout-unit
rounding. They also check the actual child glyph face, named-family
fallback, and an empty font collection's 0.5em fallback. Returning the
cascade's approximate 0.5em lengths instead of measuring the declaring
font changes word spacing to20px and indent to30px; the tests detect that.

## Root cause and remaining work

The S4v2 `dev/raikiri/src/style.rs` mapper rejects `ch` spacing, indentation
and edges before measurement. It does not supply the registered font
collection to those conversions. Neither an additional horizontal font
metrics API nor measuring the child's current style fixes inherited
values: the caller must use the retained declaring key. This is an
unconnected caller step, not a demonstrated core API gap. Both unmerged
spikes retain their original rejection behavior.

- `shodo-pn5`: wire this step into the production caller adopted by S4.
  It is a mandatory dependency of `shodo-p2m.6`, because native raikiri
  already measures plain `ch` values. It depends on S4's adoption decision.
- `shodo-e7n`: investigate `calc(2ch + 4px)` and `calc(2ch + 10%)` losing
  their declaration/provenance in the shared pinned cascade. Controls with
  plain `2ch`, `calc(4px + 10%)` and `calc(2em + 4px)` retain their values.
  This is separate from the plain-unit measurement demonstrated here.

The example deliberately covers static named fixture families, horizontal
metrics and absolute/plain-`ch` inputs. It does not implement percentage
resolution, general CSS layout, variable-axis provenance, or vertical
zero-glyph measurement. Upstream `ChFontKey` itself limits its contract to
horizontal family/size/weight/style metrics. Production adoption must
assess those additional inputs explicitly.

No pinned WPT baseline was rerun or updated for this issue. Successful
fixture tests establish the caller's numeric behavior, not a WPT PASS
increase or a fix to the unmerged spike's `Unsupported` diagnostics.
