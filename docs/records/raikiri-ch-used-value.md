# raikiri `ch` used-value wiring (shodo-pn5)

Status: adapter layer and fixture evidence done; production connection,
native comparison and WPT rerun are **not run** (blocked on S4's adoption
decision).

## Verified (fixture fonts, `cargo test -p shodo-raikiri --example ch_units`)

- Inherited `word-spacing:2ch`, `text-indent:3ch` use the declaring (parent
  Latin 20px) key: 22.88px / 34.32px. Own `margin`/`padding` `ch` use the
  child CJK 40px key: 88.8px / 111.0px per side.
- Font fallback and an empty collection (0.5em of the declaring size).
- RTL: a physical `margin-left:4ch` becomes the inline-end edge (first run
  does not move; line grows by 88.8px). Vertical and sideways writing modes
  and unknown directions are rejected.
- A `ch` factor without its declaring key is an error.
- Mutation: replacing measurement with `factor * 0.5 * size` fails 4 tests.

## Variation / orientation assessment

`ChFontKey` holds family, size, weight and style.

| Input | Reaches U+0030 advance? | Adapter behavior |
|---|---|---|
| `font-weight` | Yes: shodo's `resolve_ch` applies the weight variation of the matched face | Carried by the key |
| `font-variation-settings` | Yes (any axis) | Not in key: rejected via `require_keyed_font_inputs` |
| `font-optical-sizing:auto` (`opsz` axis) | Possibly | Unassessed; needs a fixture with `opsz` |
| `font-stretch` | Possibly (`wdth`/face selection) | Unassessed |
| vertical orientation | Yes | Rejected by `to_logical` |

Follow-ups worth filing if the production caller needs them: `opsz` and
`font-stretch` fixtures, or extending upstream `ChFontKey`.

## Not run

- Native `measure_ch_advance_for_font_key` comparison: needs raikiri's
  parley `FontContext`; the earlier probe (shodo-3fl) reported 11.44px /
  22.2px, matching shodo's measurement, but it was not rerun here.
- WPT `word-spacing-003` etc. (native and candidate): the harness lives in
  raikiri; the candidate run needs the S4-adopted caller. No baseline change
  is claimed.
