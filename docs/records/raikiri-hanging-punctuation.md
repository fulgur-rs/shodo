# Representative caller wiring of hanging-punctuation none/first

`dev/raikiri/examples/hanging_punctuation.rs` is a development-only
representative caller. It shows that raikiri's already-parsed
`hanging-punctuation: none | first` reaches shodo's `LineOptions` and
produces the native-style leading U+3000 hang. It does not adopt or change
either S4 spike, and it is not the production cutover (see Deferred).

## What is wired

`source_replay::project` (`dev/raikiri/examples/support/source_replay.rs`)
maps the IFC root's resolved `ComputedValues.hanging_punctuation` into
`LineOptions.hanging_punctuation.first`:

- `None` -> `first: false`
- `First` -> `first: true`
- any other value (the enum is non-exhaustive) -> an error, so the caller
  fails closed instead of guessing

`last`, `force_end` and `allow_end` stay `false`. Only the root's value is
used, because the property applies to the block container. Descendants'
inherited copies are not residual style: `text_style` resets
`hanging_punctuation` to its initial value on the text style so it is not
reported as an unmapped residual.

## Reproduce

The WPT checkout is a local input. CI does not need it; CI runs only the
example's tests:

```sh
cargo test -p shodo-raikiri --example hanging_punctuation
```

To replay the original static test and reference (WPT revision
`97ea26e26a2aac3eec7e770650b25e7049ed4a4e`, raikiri pin
`ab7e619a8f321f03de8b8c8b9342954868e044c8`, viewport width 800):

```sh
cargo run -p shodo-raikiri --example hanging_punctuation -- \
  "$HOME/.cache/raikiri/wpt" /tmp/hanging-punctuation-first-002.json
cmp /tmp/hanging-punctuation-first-002.json \
  dev/raikiri/data/hanging-punctuation-first-002.json
```

The committed evidence is `dev/raikiri/data/hanging-punctuation-first-002.json`
(test `css/css-text/hanging-punctuation/hanging-punctuation-first-002.html`,
reference `.../reference/hanging-punctuation-first-002-ref.html`). A second
run was byte-identical to it.

## What the evidence shows

From the committed JSON (all four `checks` are `true`, `passed: true`):

| Case | hang_start | Glyphs (advance @ inline position, cluster) |
| --- | --- | --- |
| test (`first`) | 40.0 | 40 @ -40.0 (cluster 0), 30 @ 0.0 (cluster 3) |
| control (`none` forced) | 0.0 | 40 @ 0.0 (cluster 0), 30 @ 40.0 (cluster 3) |
| reference | 0.0 | 30 @ 0.0 (cluster 0) |

- The U+3000 glyph is retained (cluster 0 is still present in the test line).
- `hang_start` equals the U+3000 advance (40.0).
- The test's down arrow (last glyph, 0.0) sits at the same inline position as
  the reference's down arrow (0.0).
- The `none` control puts the arrow at 40.0, so it does not align.

Scope, stated precisely. This is a shodo layout comparison performed in the
caller. It compares the original test's down arrow against the reference's
down arrow only; the up-arrow `↑` divs in the original pages are not measured,
so the full WPT "arrows aligned" verdict is not claimed. It is not raikiri
page paint, not a WPT PASS verdict, and does not change the baseline PASS
count. The candidate page paint remains unavailable in the frozen S4 spike.

## Semantics and limits

Covered by the nine tests in the example:

- Applies to the first formatted line only. A line after a forced `<br>` does
  not hang (`hang_start` 0.0, first glyph at 0.0).
- An absent declaration and explicit `none` do not hang; explicit `none`
  overrides an inherited `first`.
- An inherited `first` reaches the IFC root and hangs.
- A non-leading U+3000 does not move (`hang_start` 0.0, glyph at 0.0).
- The hang is a shift by the retained glyph's advance: the following glyph
  moves by exactly that advance relative to the `none` control.

Native limits and shodo extras:

- Native `raikiri-paint` hangs only a leading U+3000 in an LTR text node
  (pinned source `crates/raikiri-paint/src/text.rs`, about lines 379-390 at
  the pinned raikiri).
- shodo core additionally supports opening brackets and quotes and has its own
  RTL hanging tests (`crates/shodo/tests/japanese.rs`,
  `rtl_hanging_uses_the_inline_start_and_end_after_mirroring`).
- At the caller level, `rtl_leading_u3000_is_shodo_only_behavior` only asserts
  `hang_start > 0` for `direction: rtl`. The observed logical positions are
  identical to LTR (`hang_start` 16.0, glyph 0 at -16.0, glyph 1 at 0.0). This
  does not prove RTL mirroring at the caller level.
- RTL, quote and bracket equivalence with native is not verified at the caller
  level, those behaviors are not native-parity claims, and no WPT verdict is
  made for them.

## Deferred

- `last`, `force-end`, `allow-end` and combined values need raikiri-style
  parser support and stay in parent `shodo-9an`. The caller keeps those
  `LineOptions` flags `false`.
- Adoption of this mapping in the real production switch still waits for the
  S4 adoption policy and `shodo-p2m.6`. That dependency is not satisfied by
  this caller. Neither S4 spike was changed.
