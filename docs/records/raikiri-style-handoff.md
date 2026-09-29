# Representative caller ownership split of flow/paint CSS

`dev/raikiri/examples/support/style_handoff.rs` is a development-only
representative caller step. It takes the residual (non-text) CSS that the
frozen S4 style gate rejects with `noninitial style not mapped yet` and splits
it into typed handoffs, `Flow`, `Positioned`, `BoxPaint` and `Decoration`,
that keep the resolved values instead of resetting them to initial. The
hanging-punctuation and vertical-text residuals are accounted under
`shodo-9an.1` and `shodo-3v2`. Any residual field without an owner fails
closed with `unmapped: <field>`.

It does not paint, place positioned boxes, or run a block formatting context.
It does not adopt or change either S4 spike.

## Ownership table

`split` reruns the frozen S4 difference check on the prepared input, then maps
each reported field to an owner through `owner()`. The 14 base fields follow
`field_rules` in `dev/raikiri/data/raikiri-style-diagnostics.json`.
`outline_offset` and the four extra decoration fields (`text_decoration_style`,
`text_decoration_color`, `text_decoration_thickness`, `text_underline_offset`)
are not in the frozen reset list, so they were added to the table here. With
the three sibling-issue fields that makes 22 field names.

| Owner | Fields |
| --- | --- |
| Flow | `float`, `clear` |
| Positioned | `position`, `left`, `top`, `z_index` |
| BoxPaint | `background_color`, `background_image`, `background_position`, `background_repeat`, `background_size`, `outline`, `overflow`, `outline_offset` (added) |
| Decoration | `text_decoration_line`, `text_decoration_style`, `text_decoration_color`, `text_decoration_thickness`, `text_underline_offset` (last four added) |
| Hanging punctuation, `shodo-9an.1` | `hanging_punctuation` |
| Vertical text, `shodo-3v2` | `cssom_writing_mode`, `text_orientation` |

Which values the handoff structs retain:

- `Flow` keeps `float` and `clear`.
- `Positioned` keeps `position`, `left`, `top` and `z_index`, plus an
  `out_of_flow` flag.
- `BoxPaint` keeps `background_color`, `background_image`,
  `background_position`, `background_repeat`, `background_size`, `outline` and
  `overflow`. `outline_offset` is not retained in the struct.
- `Decoration` keeps only `text_decoration_line`. The other resolved decoration
  fields and `outline_offset` are accounted for in `residual` (so they have an
  owner and do not fail closed) but their values are not retained in a handoff
  struct. Only the solid-underline subset is converted, by `solid_underline`
  (see the limits below).

## Evidence and reproduce

The WPT checkout and the original comparison are local inputs and are not in
CI. CI runs only the example's tests (11 tests covering value retention, the
fail-closed path, the owner table, error-node parsing, the classification
count comparison and `solid_underline`):

```sh
cargo test -p shodo-raikiri --example style_handoff
```

To replay the original residual blocks (WPT revision
`97ea26e26a2aac3eec7e770650b25e7049ed4a4e`, raikiri pin
`ab7e619a8f321f03de8b8c8b9342954868e044c8`, original comparison SHA256
`67434d34bbe6928ab3a67ba43b02120b27407d57e9fb00b95af10daefc3ce01d`):

```sh
cargo run -p shodo-raikiri --example style_handoff -- \
  "$HOME/.cache/raikiri/wpt" \
  target/worktrees/shodo-s4-v2/target/s4v2/wpt-batch-full/comparison.json \
  /tmp/raikiri-style-handoff.json
cmp /tmp/raikiri-style-handoff.json dev/raikiri/data/raikiri-style-handoff.json
```

The command prints `109 documents, 303 blocks, 0 errors` and exits non-zero if
the replay is incomplete. The committed evidence
(`dev/raikiri/data/raikiri-style-handoff.json`) records:

- 109 documents and 303 blocks; `unmapped_errors` is empty; `complete` is true.
- Residual field attempts by owner: positioned 255, box-paint 161, flow 29,
  vertical-text (`shodo-3v2`) 20, hanging-punctuation (`shodo-9an.1`) 11,
  decoration 4. These count fields, not blocks: one block can report several.
- Per-field counts (`field_counts`) equal the block counts in the committed
  classification `raikiri-style-diagnostics.json` for every field
  (`field_counts_match_classification` is true): `position` 177,
  `background_color` 145, `z_index` 48, `float` 25, `left` 17,
  `cssom_writing_mode` 16, `top` 13, `hanging_punctuation` 11, `overflow` 8,
  `clear` 4, `outline` 4, `text_decoration_line` 4, `text_orientation` 4,
  and 1 each for `background_image`, `background_position`,
  `background_repeat` and `background_size`.
- `candidate_wpt_image_verdicts` is 0 and `pass_delta` is null.

The replay verifies the original resource bytes
(`verify_original_resources`) and requires the per-field block counts to equal
the committed classification. It does not re-run the parse-warning trace,
parser-resource-trace, cascade-length and ancestor-in-root checks that
`raikiri_style_diffs.rs` performs.

## Semantics and limits

- `out_of_flow` is true for `absolute` and `fixed`. The values are kept; it is
  not a drop. A single paragraph may legitimately ignore such a box, but a
  whole page must still place it.
- A node's own solid underline converts through `solid_underline` to
  `shodo::style::TextDecoration` for `PaintStyle.underline`. Overline,
  line-through, blink, spelling/grammar-error lines, non-solid styles, an
  unsupported decoration color and a non-auto underline offset are rejected.
  `solid_underline` is exercised only by tests in this step; the replay CLI
  does not call it.
- Propagation of an underline to inline descendants and the single drawing of a
  shared glyph remain the existing caller path (`raikiri_contracts.rs`,
  `shodo-v7f`) and are not re-proved here.
- This record makes no claim about box-painting correctness, z-order
  rendering, clipping, gradients, whole-page layout, native/baseline
  comparison, WPT PASS/FAIL, image verdicts or whether a production cutover is
  necessary. Those stay with the S4 adoption decision and `shodo-p2m.6`.

## Deferred

- Actual background, outline and gradient painting.
- Applying relative offsets and stacking order.
- Float and clear placement through Taffy `FloatContext` and shodo's
  `FloatCursor` / `LineConstraint`.
- Overflow clipping.

Neither S4 spike was changed by this work.
