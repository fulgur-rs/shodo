# Lines limited by character count

`Paragraph::break_all_with_grapheme_limit` lays out a paragraph greedily with
both an inline width and a maximum number of characters per line:

```rust
let lines = paragraph.break_all_with_grapheme_limit(
    &mut layout_context,
    &line_options,
    240.0,
    20,
    &atomic_sizes,
);
```

The count uses **Unicode extended grapheme clusters in the processed paragraph
text**. It does not count UTF-8 bytes, Unicode scalar values, glyphs, or the
original source text. Combining sequences and ZWJ emoji count as one. An atomic
inline and a preserved tab each count as one. Collapsed whitespace counts as
it appears in the processed text. A forced line break ends its line even when
the count is below the limit. Transparent layout controls and float anchors do
not consume the count.

The limit creates a possible break after each safe grapheme boundary. The
width can end a line sooner, and a forced break always ends it. The line may
contain more than the requested count when the next boundary is indivisible,
such as a text transformation expansion, a combined-text span, or a shaping
group that cannot be safely split. A limit of zero still consumes the first
indivisible group so line iteration makes progress. With `::first-line`, the
first line counts its transformed text; its continuation token maps back to
the normal paragraph text. `Line::text_range()` on that first line refers to
the alternate text, as with ordinary `::first-line` layout.

Use `LineConstraint::max_graphemes` with `Paragraph::next_line` or
`Paragraph::lines` when you place floats yourself or apply block-height
limits. Keep the same limit on retries and continuation lines. Width-only
`BreakPlan`s cannot encode this character limit; supplying one falls back to
greedy layout with a warning. The normal line result still carries the source
mapping, glyph runs, bidi order, and hit-test data for its selected range.

This is shodo's processed-grapheme counting contract, not a Parley-compatible
character-count mode.
