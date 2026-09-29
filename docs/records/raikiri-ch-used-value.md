# raikiri `ch` used-value wiring (shodo-pn5)

Status: adapter layer and fixture evidence done; production connection,
native comparison and WPT rerun are **not run** (blocked on S4's adoption
decision).

## Verified (fixture fonts, `cargo test -p shodo-raikiri --example ch_units`)

- Inherited `word-spacing:2ch`, `text-indent:3ch` use the declaring (parent
  Latin 20px) key: 22.88px / 34.32px. Own `margin`/`padding` `ch` use the
  child CJK 40px key: 88.8px / 111.0px per side.
- Font fallback and an empty collection (0.5em of the declaring size).
- RTL (box and paragraph both RTL): a physical `margin-left:4ch` becomes the
  box's inline-end edge and trails the content (first run does not move, the
  line grows by 88.8px); inherited `text-indent:3ch` offsets the first run by
  34.32px. Vertical and sideways writing modes, unknown directions, and a box
  direction that differs from its paragraph's are rejected (shodo swaps such
  a box's edges when drawing; that placement was not verified against CSS).
- A `ch` factor without its declaring key is an error. (A value with no
  factor at all passes through as the cascade px; `calc()` provenance loss
  is shodo-e7n.)
- Mutation: replacing measurement with `factor * 0.5 * size` fails 4 tests.

## Variation / orientation assessment

`ChFontKey` holds family, size, weight and style.

| Input | Reaches U+0030 advance? | Adapter behavior |
|---|---|---|
| `font-weight` | Yes: by code reading, `resolve_ch` applies the matched face's weight variation; no variable-font fixture test exists yet | Carried by the key |
| `font-variation-settings` | Yes (any axis) | Rejected by `require_keyed_font_inputs`, but only for the `ComputedValues` passed in (the fixture checks parent and child). A declaring ancestor that is not checked can still under-reject; a real caller must check the declaring element |
| `font-optical-sizing:auto` (`opsz` axis) | Possibly | **Fails open**: `resolve_unit` never applies `opsz`; needs a fixture |
| `font-stretch` | Possibly | Not exposed by the pinned cascade and `FontQuery.width` stays 100: **fails open** |
| language / script | Fallback selection may differ | `ChFontKey` has no `lang`; queries use default script Latn, no language |
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
