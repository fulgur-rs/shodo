# Custom line-break opportunities

`ParagraphBuilder::with_line_break_override` and `RichText::with_line_break_override` let a caller add or suppress soft line-break opportunities while building a paragraph. The callback runs once per eligible boundary during `build`; line layout, intrinsic width measurement, `BreakPlan`, and subsequent layout retries all use the resulting immutable opportunities. Omitting the callback preserves the existing behavior. Returning `UseStandard` at every boundary also preserves it.

```rust
use shodo::font::FontCollection;
use shodo::limits::Limits;
use shodo::style::ParagraphStyle;
use shodo::{LayoutContext, LineBreakOverride, RichText};

let style = ParagraphStyle::default();
let limits = Limits::default();
let fonts = FontCollection::new(&limits);
let paragraph = RichText::new(&style)
    .with_line_break_override(|context| {
        if context.text[..context.offset].chars().next_back() == Some('/') {
            LineBreakOverride::Allow
        } else {
            LineBreakOverride::UseStandard
        }
    })
    .push("path/to", &style.root)
    .build(&mut LayoutContext::new(), &fonts)
    .unwrap();
```

`LineBreakContext::text` is the paragraph text **after** white-space processing and `text-transform`; `offset` is a UTF-8 byte boundary in that text. It is not a byte offset in the caller's original DOM text. The processed text can contain bidi controls inserted for inline boxes. The callback can inspect the full string on either side of the boundary and receives its standard classification (`Prohibited`, `Allowed`, `Emergency`, or `Hyphen`). It runs only at internal Unicode grapheme boundaries. A `::first-line` alternate style builds its own processed text and can invoke the same callback again with different offsets. Callbacks should therefore be deterministic for a given context and should not depend on call count.

| Existing condition | Override behavior |
| --- | --- |
| Mandatory break, including a preserved newline or forced break | The callback is not called; the break stays mandatory. |
| `text-wrap-mode: nowrap` | The callback is not called; no new soft break is added. |
| Inside one grapheme, an indivisible transformed source character, or a `text-combine-upright` span | The callback is not called; the boundary remains unavailable. |
| Other internal grapheme boundary | `UseStandard` keeps the Unicode/CSS decision; `Allow` makes an ordinary soft break; `Prohibit` removes a soft break, including an emergency or hyphen opportunity. |

`Allow` does not insert a hyphen. It may replace a standard hyphen opportunity with an ordinary one. The callback changes opportunities, not the physical width of a line: width, float, block-height, glyph, bidi, and hit-test processing still use the normal layout pipeline. A narrow line without an allowed break can overflow, just as it does with `overflow-wrap: normal`.

The callback is applied before shaping units are retained. It is not retained by the `Paragraph`, and it is never rerun during layout or intrinsic measurement. Each `Paragraph` has its own identity, so a `BreakPlan` or layout cache entry for a paragraph with one override cannot be applied to another paragraph with different opportunities.

This API provides the caller hook for specialized ASCII rules. It does not ship a Chromium compatibility table: such a table requires a named upstream version and explicit compatibility tests, and Chromium's rules are outside shodo's default Unicode/CSS behavior. A caller can supply a versioned table through this callback when that compatibility is needed.
