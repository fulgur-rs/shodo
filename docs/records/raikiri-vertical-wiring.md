# raikiri vertical writing-mode / text-orientation wiring (shodo-3v2)

Status: adapter and fixture evidence done; production connection, native
image comparison and WPT baseline movement are **not measured** (they need
the caller S4 adopts). Neither S4 spike was modified.

## What was wired

`dev/raikiri/adapter/vertical.rs` maps raikiri computed values to shodo:

| raikiri | shodo | Notes |
|---|---|---|
| `cssom_writing_mode` | `ParagraphStyle.writing_mode` | Must be `cssom_writing_mode`: the renderer-facing `writing_mode` is normalized to horizontal and silently drops vertical text |
| `text_orientation` | `InlineStyle.text_orientation` | `mixed`/`upright`/`sideways` 1:1 |
| `text_combine_upright` | `InlineStyle.text_combine_upright` | `none`/`all` 1:1 |
| `text_autospace` | `InlineStyle.text_autospace` | Only `normal` and `no-autospace`; `auto` and explicit boundary sets are rejected (no shodo equivalent) |

All raikiri enums are `#[non_exhaustive]`; unknown values are errors.

`dev/raikiri/examples/vertical_wiring.rs` runs the cascade → adapter → shodo
path on the four original documents, vendored unmodified in
`dev/raikiri/inputs/vertical/` (SHA-256 checked against
`raikiri-style-diagnostics.json`). The WPT reference is the oracle. The
container's `cssom_writing_mode` selects the paragraph mode; each child `div`
is one paragraph and each `span` an inline box with its own mapped style.
The documents name no font, so the caller supplies the family. CI tests use
the fixture CJK face; it has no glyph for 国 (U+56FD), so there every 国 is
`.notdef` and those comparisons are notdef-against-notdef (the 60px widths
hold only because `.notdef` is 1em wide). The same oracle therefore also runs
against the original 88-font registry (the pinned loader in
`examples/support/source_fonts.rs`, generic `serif` as the documents' initial
family) when `SHODO_WPT_ROOT` points at a WPT checkout; it is skipped in CI.
With the pinned registry there is no `.notdef` glyph in any of the four
documents.

## Results (`cargo test -p shodo-raikiri --example vertical_wiring`)

- `text-autospace-vertical-combine-001`: **matches its reference** per glyph,
  font and orientation with both the fixture face and the pinned registry
  (2 lines, 60px each = 国 + one combined 20px em + 国). Its two-character
  `.tcy` spans reach shodo as `Combined` runs. This is shown for these spans
  only: shodo declines to combine some candidates and reports them as
  warnings (see below).
- `text-autospace-vertical-upright-001`: all glyphs are `Upright` and it
  **matches its reference** (`upright_test_matches_reference`). CSS Text 4
  excludes non-ideographic letters and numerals from the autospace classes
  when they are upright in vertical text; shodo-39u made
  `crates/shodo/src/line/autospace.rs` use the same per-character orientation
  resolver as shaping, so each line is 60px like the `no-autospace` reference
  (it was 65px before). Horizontal and sideways modes are unchanged.
- Vertical is not dropped: forcing the same document to `horizontal-tb`
  yields `Horizontal` runs. Mapping `vertical-rl` to horizontal in the
  adapter was tried and failed 4 tests.
- `text-autospace: auto` is rejected (not treated as `normal`).

## TCY limits

The adapter maps `text-combine-upright: all` 1:1 and does not check that the
content meets shodo's combine conditions. shodo declines some candidates
(`crates/shodo/src/analysis/combine.rs`, where adjacent `all` candidates split
by a box boundary in one scope are rejected), so e.g.
`<span style="text-combine-upright:all"><b>1</b><b>2</b></span>` is laid out
as ordinary text. shodo-trj makes shodo report each rejected candidate as a
`WarningKind::Unsupported` warning naming its processed-text byte range, so a
caller can read `Paragraph::warnings()` and refuse to treat the paragraph as
composed. Other reasons a candidate is not composed are not diagnosed.

## Classification (provisional; native and baseline not measured)

- Adapter wiring for `cssom_writing_mode`, `text-orientation`,
  `text-combine-upright`: likely required for the production switch, since
  the current root construction and guard reject the 16 vertical blocks /
  4 documents found by shodo-zt3. Re-judge after native is measured.
- shodo-39u (autospace on upright vertical text): fixed in shodo core; the
  document now matches its reference.
- `text-autospace: auto` and custom boundary sets are unsupported and fail
  closed.

## Not measured

- Native raikiri image output and the original WPT baseline for these
  documents; no baseline increase or decrease is claimed.
- horizontal LTR/RTL behavior was covered only by the existing `ch_units`
  tests, which stay green; no broader horizontal regression run was done.
- Other silently dropped declarations are not proven by this work.
