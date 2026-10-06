# Full hanging-punctuation caller projection (shodo-9an)

Raikiri prerequisite [PR 610](https://github.com/fulgur-rs/raikiri/pull/610)
implements the CSS Text 3 grammar
`none | [first || (force-end | allow-end) || last]`, stores all twelve valid
keyword sets, serializes in first/end/last order, and maps every flag through
its real IFC root and inline styles. Shodo's development caller now pins its
immutable merge `fe9aea9ade56ff046d38a6dddd31d02303466bfc`.
The old single-keyword enum variants remain valid callers. Duplicate keywords,
conflicting end modes and combinations with `none` are rejected.

The prerequisite's declaration parser preserves `!important` for the enclosing
parser. Tests cover all twelve important declarations and their actual cascade
priority and inheritance. Its existing CSS-wide `inherit !important` limitation
is unrelated to this change and remains recorded in PR 610. Normal `inherit`
and implicit inheritance retain the complete set.

## Box ownership

`source_replay` uses one complete mapper for `first`, `last`, `force_end`, and
`allow_end`. The root delegates its computed flags to `LineOptions`; its
`InlineStyle.hanging_punctuation` is `None`. Each nested inline uses
`Some(computed_flags)`, including `Some(default())` for CSS `none`. Thus an
inline can enable, inherit, or clear hanging independently of the root.
Existing source text, glyphs, DOM offsets and paragraph mapping are retained.

The first-002 example's `force_none` diagnostic control replaces root options.
It affects root-level text; inline computed overrides remain active. A focused
regression fixes this scope. It is not a global CSS rewrite.

## Original WPT inputs

Five [unaltered WPT files](../../dev/raikiri/inputs/hanging-punctuation/README.md)
from revision `97ea26e26a2aac3eec7e770650b25e7049ed4a4e` are included with their
original Ahem stylesheet/font and upstream license. Tests verify document
SHA-256 values, parse the original resources, map every noninitial hanging set,
and verify the bytes remain unchanged after replay. The actual merged Raikiri
native IFC assignment accepts all five original documents.

| Original case | Hanging flags |
| --- | --- |
| last / last-whitespace | last |
| first-and-last-together | first, last |
| force-end-001 | force_end |
| allow-end-001 | allow_end |

The original force/allow `.test` roots also build through `source_replay`.
The last/combined pages retain their other unsupported paint/flow fields;
source-replay errors name those fields, including `color`, and the tests forbid
hiding the original green text color. Hanging is no longer an unmapped residual.
This acceptance is about grammar and style/IFC assignment. The small copied
inputs omit the force/allow pages' Japanese IPA fonts. These tests do not compare
reference pixels, establish original-font geometry, or count WPT PASS results.
Neither frozen S4 spike, original reference expectations nor tolerances change.

## Pin compatibility and font provenance

The new native API exposes an opaque Shodo font collection and IFC positioned
lines. `first_line_wpt` now supplies that collection to the document, reads
`PositionedLines` instead of the removed Parley text layout, and propagates
native paint errors. Runtime output identifies the actual merge pin; frozen
input diagnostics and their historical revisions remain separate fields.
The prior [first-line](raikiri-first-line.md) and
[none/first](raikiri-hanging-punctuation.md) measurements remain historical.

`source_fonts` reproduces the immutable native WPT registration policy from
Raikiri `d7dabe7` (`fonts::walk_fonts` and `layout::ifc::font::wpt_collection`):

- Sort paths; register every exact `Ahem.ttf` basename first, preserving duplicates.
- Accept case-insensitive TTF/OTF extensions; skip symlinks/nonregular/oversized files.
- Bound each file to 100 MiB and use public `read_bounded_contained_file` for containment.
- Propagate I/O errors; skip rejected reads and unregistrable fonts.
- Require a registered Ahem file and a nonempty family list.
- Read first-face typographic/family names, deduplicate in registration order, and
  give all six generics that same list; disable system fonts.

Reported byte hashes and generic order describe this caller's faithful policy
mirror, rather than enumeration of the opaque native runtime. Focused tests
check preferred duplicates, bytes, all six orders, missing/corrupt Ahem failures,
and compare actual native IFC vs mirrored generic font selection for Latin/CJK.
No public Shodo font API or global dependency patch is introduced.

## Verification

Meaningful RED/GREEN covers missing inline `none` projection and rejected root
`last`. Focused coverage includes all twelve root/inline states, retained glyphs
at both edges, inline overrides/clearing/inheritance, first-line behavior,
force-end vs allow-end fitting, and the root-only control.

Prerequisite exact source commit `92cef905158bd63fa3549632924a8596fa863313`
passed all five CI results on Rust 1.91. Auxiliary style docs passed before/after
with and without `--cfg test`; DOM private docs passed before/after. DOM's test
configuration stops on the same preexisting missing `tempfile` E0433, so it is
excluded according to foreign AGENTS.md. Local full workspace on Rust 1.97.1
stopped at an unchanged Unicode table invariant after 5442 passed, one failed,
one ignored (39 completed suites): U+11B61 is alphabetic in 1.97 and not in 1.91.
It is not recorded as a passing local workspace run. Workspace/private and
HTTP-feature rustdoc passed locally. The auxiliary comment lint has three
unchanged baseline findings and no added finding.

Shodo verification used Rust/Cargo 1.97.1, a fresh dedicated target directory,
and two build jobs. The following completed successfully:

- `cargo test --workspace --locked`: 96 completed suites, 1659 passed,
  zero failed, nine ignored.
- `cargo test -p shodo-raikiri --example hanging_punctuation --locked`:
  24 passed, including original resource hashes and the actual block/inline
  display profiles.
- `cargo test -p shodo-raikiri --all-targets --locked`: 12 completed suites,
  141 passed, zero failed or ignored.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked`.
- `cargo fmt --all -- --check` and `git diff --check`.

The full workspace test ran on `c081cfb`; the subsequent focused/all-targets
runs, clippy and docs include the final original-input test strengthening and
`hang_end` diagnostic JSON field. The latter fixes an example's non-test
dead-code error: both hanging amounts now appear in its measurement output.

Both representative runtime commands completed successfully against original
WPT `97ea26e26a2aac3eec7e770650b25e7049ed4a4e` and the merge pin above:

```sh
cargo run -p shodo-raikiri --example hanging_punctuation --locked -- \
  "$WPT_ROOT" "$OUTPUT_DIR/hanging-punctuation.json"
cargo run -p shodo-raikiri --example first_line_wpt --locked -- \
  "$WPT_ROOT" "$OUTPUT_DIR/first-line"
```

The first command reproduced all four original first-002 checks: glyph
retention, leading-advance hanging, reference arrow alignment and the
non-aligning `none` control. Its JSON includes both hanging amounts.

The migrated first-line diagnostic verified the original 88-font inventory,
registered 88 distinct byte hashes and reported native Shodo IFC positioned
lines. These are new measurements with the current native engine, separate
from the historical Parley measurements:

| Original test | Caller classification | Native/reference exact | Caller/reference exact |
| --- | --- | --- | --- |
| `first-line-001.xht` | inline-match | false | true |
| `first-line-pseudo-021.xht` | inline-match | false | true |
| `first-line-opacity-001.html` | unsupported opacity | true | unmeasured |
| `first-line-inherit-003.xht` | unsupported first-line descendant structure | true | unmeasured |

Every row retains `counted_as_pass: false`; `full_page_wpt_passes` remains zero.
The changed native API is exercised without converting partial inline matches
or remaining unsupported styles into conformance claims.
