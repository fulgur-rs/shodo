# Japanese horizontal layout

The horizontal line engine implements Japanese line-break tailoring,
punctuation spacing, hanging punctuation and inter-character justification.
These decisions affect line fitting, intrinsic sizes and final glyph placement;
the renderer consumes the accepted output.

The reference is [CSS Text4 WD2026-08-14](https://www.w3.org/TR/2026/WD-css-text-4-20260814/)
and [JLREQ §3.1.11](https://www.w3.org/TR/jlreq/#character_sequences_which_do_not_allow_increase_of_spacing_as_part_of_line_adjustment_processing).
This implementation does not certify WPT conformance. The separate browser
endpoint comparison is described in [browser-comparison.md](browser-comparison.md).

## Input and options

Register the actual Japanese font bytes in `FontCollection`, put its registered
family in the computed `InlineStyle::font_families`, and set `lang` to `ja` or
`ja-JP`. Inline styles are computed values supplied by the caller.

```rust
use shodo::style::{
    FontFamily, InlineStyle, LineBreak, LineOptions, ParagraphStyle,
    TextAlign, TextJustify, TextSpacingTrim,
};

let style = ParagraphStyle {
    root: InlineStyle {
        font_families: vec![FontFamily::Named("Your Japanese face".into())],
        font_size: 16.0,
        lang: Some("ja".into()),
        line_break: LineBreak::Strict,
        text_spacing_trim: TextSpacingTrim::Normal,
        ..Default::default()
    },
    ..Default::default()
};
let options = LineOptions {
    text_align: TextAlign::Justify,
    text_justify: TextJustify::InterCharacter,
    ..Default::default()
};
```

`Strict`, `Normal` and `Loose` preserve their CSS differences. In particular,
small kana and the prolonged sound mark cannot start a line in Japanese Normal
or Strict; Loose permits their normal typographic boundaries. `nowrap`,
`anywhere`, `keep-all`, grapheme ownership and indivisible transforms retain
their existing precedence.

## Punctuation spacing

| Value | Line-start opening punctuation | Line-end closing punctuation | Interior |
| --- | --- | --- | --- |
| `SpaceAll` | Keep | Keep | Keep |
| `Normal` (default) | Keep | Trim when needed to fit | Collapse specified adjacent pairs |
| `TrimStart` | Trim | Trim when needed to fit | Collapse specified adjacent pairs |
| `SpaceFirst` | Keep first/forced heads; trim soft heads | Trim when needed to fit | Collapse specified adjacent pairs |
| `TrimBoth` | Trim | Trim | Collapse specified adjacent pairs |
| `TrimAll` | Trim | Trim | Remove each eligible punctuation blank once |
| `Auto` | Trim | Trim | Uses the `TrimBoth` policy |

Spacing uses the actual fallback face, adjusted font size and variation
coordinates. The engine removes a half-advance blank on opening/closing
punctuation, or quarter-advance blanks on each side of middle punctuation, only
when the retained outline bounds leave enough room. Proportional punctuation
and synthetic emboldened/skewed instances retain their spacing. Glyph shapes
are preserved. The default `Normal` now performs real punctuation spacing, so
existing paragraph widths and wrapping can change.

Japanese comma/stop glyphs are closing punctuation; colon/semicolon glyphs are
middle punctuation. Chinese script/region subtags select the corresponding
colon/dot convention; an explicit Hans/Hant script takes precedence over the
region. Borders, padding and intervening advances prevent adjacent collapse;
zero-width source/inline markers preserve typographic adjacency. Mirrored RTL
brackets use their actual physical glyph class and blank side.

## Hanging and accepted geometry

Set the fields of `LineOptions::hanging_punctuation`:

- `first` hangs one eligible opening mark on the first formatted line.
- `last` hangs one eligible ending mark on the final line.
- `force_end` excludes a comma/stop's remaining advance even if it fits.
- `allow_end` excludes only the part of a comma/stop that exceeds the available width.

For the fixture's 16px fullwidth advances, `日本、` naturally measures48px.
With `SpaceAll`, `allow_end` and44px available, the accepted line measures44px
and reports `hang_end()==4`; at48px available the hang is zero. With `force_end`
the measure is32px and the hang is16. If trimming already removed8px from the
punctuation, only its remaining8px can hang. `force_end` takes precedence if a
caller sets both end flags.

If one typographic character qualifies for both `first` and `last`, the start
deduction takes priority and the end uses only its remaining advance. A single
quote therefore has a zero measure, rather than a negative one.

`Line::inline_size()` excludes punctuation hangs; `hang_start()`/`hang_end()`
report actual excluded amounts, including eligible trailing whitespace.
Glyphs, carets and source mappings remain present. `overflow_rect()` includes
the positioned ink. Inline borders/padding block edge hanging; margins alone
do not. With `box-decoration-break: Slice`, a border/padding blocks only the
fragment where its actual edge occurs; `Clone` repeats that edge on every
fragment. The same rule governs line-edge trimming. Start/end are logical
edges in RTL paragraphs.

Conditional end hanging is excluded from min-content but retained in
max-content. Forced hanging is excluded from both. First-line alternatives,
height retries, float width changes and planned breaks use the same rules.

## Inter-character justification

Japanese justification does not expand either side of brackets, commas/stops,
middle dots, dividing punctuation, hyphens or Japanese fullwidth spaces.
Consecutive inseparable ellipses and long dashes stay together. Ordinary
Han/kana boundaries receive the extra width. For example, `日「日本」語` with
`SpaceAll` at16px and128px available puts all32px of expansion between the inner
`日` and `本`; the bracket neighbors retain their natural spacing.

The same filter applies inside shared/owned shaping clusters and between them.
Cursive joins, variation selectors, combining continuations and indivisible
transforms are not split. Explicit other-language styles retain Western
inter-character expansion. Without a language, Han/kana/fullwidth context uses
the Japanese exclusions. Lines with no permitted opportunity use the existing
`text-align-last` fallback. This expansion policy is independent of the
line-break prohibition table.

The fixed-font snapshots cover trimming, first hanging inside a24px indent,
forced and partial end hanging, and bracket-aware justification. Snapshot
geometry records the accepted measure and hang amounts. Gray guides mark the logical content edges; ordinary checks never
update expectations.
