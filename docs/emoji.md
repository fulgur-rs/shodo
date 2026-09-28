# Emoji layout and caller color drawing

shodo selects and shapes fonts, retains accepted glyphs and lays them out. The
caller rasterizes the output. The development example demonstrates real
CBDT/CBLC PNG color emoji together with monochrome outlines; it does not add a
renderer or emoji font dependency to the root library.

## Real fixtures and reproducibility

The optional [fixture loader](../dev/fixtures/README.md#optional-real-emoji-fixtures)
registers fixed, renamed OFL1.1 Noto Color Emoji and variable Noto Emoji faces.
The [manifest](../dev/fixtures/assets/manifest.json) records immutable source
commits, source/subset SHA256 values, licenses and separate subsetting corpora.
`load_emoji_fonts` keeps original face registration IDs and disables system font
scanning. Ordinary `load_fonts` consumers retain their original three fonts.

The fixed corpus exercises 😀, ☺︎/☺️, 👍🏽, 👩‍💻, 👨‍👩‍👧‍👦, 🇯🇵, 1️⃣ and the
Scotland tag flag (`1F3F4 E0067 E0062 E0073 E0063 E0074 E007F`). These fonts
cover the tested representatives, not every Unicode emoji: the color source is
the pinned Unicode13.1 release. All five subsets reproduce with FontTools4.61.1
from verified original bytes; original three subsets remain byte-identical.

## Matching, composition and missing output

Font selection covers the entire extended grapheme. Auto presentation uses
VS15 for text, VS16 for emoji, then Unicode Emoji_Presentation. Color-table
presence helps rank eligible faces; it does not prove the caller can render that
format. `FontQuery::presentation` also provides Text/Emoji preferences for direct
matching. `InlineStyle` does not expose a CSS font-variant-emoji property; paragraph
selection uses automatic presentation. The two fixture faces share a family;
mono wght300..700 is registered with its actual range. CSS style matching precedes
presentation preference, so requesting a weight available only in mono can choose
that face even for an emoji-default character.

Cmap coverage is nominal eligibility, not a guarantee of sequence composition.
The selected face's GSUB determines the result. An unsupported sequence can
retain component glyphs in one face; the matcher does not retry candidates on the
assumption that every valid emoji must produce exactly one glyph. A font can use
multiple glyphs for a legitimate representation. Tests use both an unsupported
`😀 ZWJ 😀` sequence and a real font with its GSUB record removed; component output
keeps one editable/breakable grapheme. This is not a complete RGI sequence support
API or a claim of full CSS browser matching parity.

When no configured face covers a grapheme's nominal characters, the paragraph
records `WarningKind::Unsupported` for missing fonts and emits glyph ID0 (`.notdef`)
using the existing primary-face fallback data. Source offsets and the grapheme
remain intact. System discovery is a caller option; the fixed fixtures never
silently obtain an installed emoji font. A missing picture is not a successful
emoji rendering, and no sequence-specific replacement picture is fabricated.

Ordinary and emergency breaking, logical/visual caret movement, hit testing and
selection preserve the grapheme boundaries. Normal breaking can prohibit a break
between numeric keycaps, causing the whole pair to overflow a narrow line; emergency
wrapping can break between them, never within either. Japanese/Latin mixed text
uses the selected fonts' advances and metrics and the configured autospace policy.
Tests verify UTF8 source offsets, DOM-node splits and actual selected instances.

## Draw accepted glyphs

For each `Fragment::GlyphRun`, use `GlyphRunView::font_data()` for retained font
bytes and face index, `font()` for collection identity, `font_size()` and
`normalized_coords()` for the actual instance, and `glyphs()` for IDs and advances.
`glyph_origin(index)` and `glyph_transform()` describe placement in logical
inline/block coordinates. Apply the line's writing mode and direction through
`PhysicalConverter`, including its block offset and the caller's canvas origin.
The public local glyph matrix uses x-right/y-down; outline rasterizers often need
a y flip. Bitmap PNG coordinates already use y-down and must not be flipped again.

The shared [caller painter](../dev/fixtures/examples/support/glyph_paint.rs) first
looks up the accepted glyph in a CBDT strike. It chooses a suitable strike through
skrifa, scales bitmap pixels by font size/strike ppem, applies the strike bearings,
then the accepted physical glyph transform. Bitmap advance and PNG dimensions
never replace core advance, baseline or line height. The nominal space glyph in
a bitmap-only face can have no image; it contributes no ink and retains its accepted
advance. A missing visible bitmap still needs a supported outline or returns an
error. Bitmap painting takes no source string and cannot reshape the paragraph.

PNG decoding preserves intrinsic colors and produces premultiplied RGBA8 once;
`run.paint_style().color` remains the color for outline text and decorations.
Changing body text color does not tint CBDT pictures. Underlines/strikes use the
existing accepted source rectangles. The example handles horizontal and upright
vertical output through the public transform; it rejects synthetic weight/skew.

| Font data | Core handoff | Development painter |
| --- | --- | --- |
| CBDT/CBLC | Retained font bytes and accepted glyph/instance/placement | PNG bitmap strikes, top-left bearings |
| COLR/CPAL v0/v1 | Same handoff | Requires a caller palette/paint-graph backend |
| sbix | Same handoff | Requires a caller sbix bitmap backend |
| SVG glyph data | Same handoff | Requires a caller SVG backend |
| glyf/CFF outlines | Same handoff | Existing skrifa outline path |

The sample rejects BGRA/mask encodings, non-CBDT bitmap placement, malformed PNG,
metrics/PNG dimension disagreement, zero/nonfinite sizes/ppem/transforms and
clipped output on a declared canvas. It bounds decoded glyph images to1,048,576
pixels (4MiB RGBA) and validates IHDR dimensions before allocating decoded output.
Other color formats with an outline can use that outline path in monochrome;
their intrinsic color representation is not rendered by this sample.
It is a fixed-font example, not a general CSS painter or a color-format capability
negotiation API. Applications choose backends appropriate for their fonts.

## Run the example

From the repository root, choose an explicit destination:

```sh
cargo run -p shodo-fixtures --example emoji_png -- /tmp/shodo-emoji.png
```

The PNG contains all representative sequences mixed with Japanese/Latin. Standard
output identifies accepted glyph IDs, font identities, sizes, variation coordinates,
origins and advances, plus a nonzero intrinsic-color pixel count. Tests verify real
color pixels, supplied glyph ID/origin changes, bitmap bearings, alpha, vertical
placement, spaces/selectors, ordinary text colors/decorations and failure cases.
CI runs the same example on stable and Rust1.89. Root wasm compilation remains
independent of this development renderer and its font assets.

Format references: [OpenType CBDT](https://learn.microsoft.com/en-us/typography/opentype/spec/cbdt),
[CBLC](https://learn.microsoft.com/en-us/typography/opentype/spec/cblc),
[COLR](https://learn.microsoft.com/en-us/typography/opentype/spec/colr),
[SVG](https://learn.microsoft.com/en-us/typography/opentype/spec/svg).
