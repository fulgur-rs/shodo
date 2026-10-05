# shodo-zb0.7: per-window shaping context and face preparation

`crates/shodo/src/shape.rs::shape_inputs` (reached from
`shape_items_with_base_scopes`) splits each shaping input into windows of at
most `max_shaping_run_bytes` (or a narrower ruby base scope budget). Each
window used to:

- collect `pre`/`post` context `String`s from up to five neighbouring scalars,
  even though `shape/input.rs` already has a five-scalar inline `Context`;
- rebuild `harfrust::FontRef`, look up `ShaperData` through the font cache,
  build a `Shaper`, re-parse the face with skrifa for the upright
  CFF-without-VORG check, re-parse `style.lang`, and start an empty
  `cff_origin_deltas` map.

All of these values are fixed for one input, so this work only costs extra
when an input spans more than one window.

Two candidates were measured as separate A/B steps:

- **A (`ctx`, 02258e0)**: inner window edges use `input::Context::from_chars`
  (a `[u8; 20]` inline buffer) and are built only when the window does not
  reach the input edge. The outer edges still use `input.before`/`after`.
  Semantics are unchanged: `pre` still takes only `scalars[start-5..start]`
  without chaining `input.before`.
- **B (`prep`, daef4bf)**: the face reference, shaper data, shaper, skrifa
  CFF/VORG check, parsed language and the CFF origin delta cache are prepared
  once per input. The missing-font path is unchanged. The plan cache key does
  not depend on per-window shaper state.

The origin delta cache is declared inside the per-input loop. Each input has a
single resolved `RunInstance` and variation coordinates, so cached deltas are
never shared across font instances or coordinates. The cache can now hold one
entry per distinct glyph in the input rather than per window. It is still
bounded by the face's glyph count and freed at the end of the input.

## Probe

`dev/bench/examples/shape_window_prep.rs` builds the paragraphs below with
fixed fixture fonts. Every text case repeats its string 64 times. Inputs are
chosen to stay a single shape item: a space would select the Latin face in the
Arabic text, and kana would split the Han text by script.

| case | content | budget |
| --- | --- | --- |
| latin-single / latin-split16 | Latin sentence | default / 16 bytes |
| arabic-single / arabic-split16 | Arabic, no spaces (joining across windows) | default / 16 |
| cjk-upright-vorg-{single,split16} | Han, `vertical-rl`, CFF face with VORG | default / 16 |
| cjk-upright-novorg-{single,split16} | Same face with VORG removed (vmtx/CFF-bounds origin path) | default / 16 |
| ruby-base-split6 | 64 ruby containers, base content limits `max_shaping_run_bytes=6` | paragraph default |
| ruby-base-glyph-limit | Same, plus base `max_shaped_glyphs=4` | `LimitExceeded` path |

Modes: `digest` (output hash), `time` (21 samples, median), `alloc`
(`CountingAllocator`, one build in scope) and `loop <case> <n>` for valgrind.
The digest covers line ranges and sizes, run source ranges, face bytes and
index, normalized coordinates, bidi level, orientation, font size, glyph
id/cluster/position/block offset/advance, glyph origin, ruby annotation
lines, and the paragraph plus break warnings. The probe asserts that the
digest is stable across repeated builds.

## Equivalence

All 10 cases produce byte-identical digests and warning lists in `base`, `ctx`
and `prep`. This includes the ruby glyph-limit failure
(`LimitExceeded { kind: ShapedGlyphs, limit: 4, actual: 5 }`), whose `actual`
depends on window boundaries.

A new unit test,
`shape::tests::missing_vorg_origin_cache_spans_budget_windows_but_not_inputs`,
covers the origin cache scope. It uses a no-VORG CJK face, two inputs with
different font sizes and one Han scalar per window. It asserts that glyph
ids, advances and both offsets match the unsplit build, that there are no
warnings, and that exactly 4 delta evaluations happen (2 distinct glyphs × 2
inputs). Before B the test sees 10 evaluations, one per window.

## Allocations (`CountingAllocator`, one build)

| case | calls base → ctx → prep | gross bytes base → ctx → prep |
| --- | ---: | ---: |
| latin-single | 303 → 303 → 303 | 2,199,022 → 2,199,022 → 2,199,022 |
| latin-split16 | 829 → 471 → 471 | 1,876,638 → 1,873,774 → 1,873,774 |
| arabic-split16 | 1,003 → 431 → 431 | 874,552 → 867,688 → 867,688 |
| cjk-upright-vorg-split16 | 1,282 → 466 → 466 | 670,416 → 660,624 → 660,624 |
| cjk-upright-novorg-split16 | 1,699 → 883 → 477 | 708,923 → 699,131 → 661,435 |
| ruby-base-split6 | 16,466 → 16,082 → 16,082 | 2,714,313 → 2,710,217 → 2,710,217 |
| ruby-base-glyph-limit | 1,589 → 1,583 → 1,583 | 446,646 → 446,582 → 446,582 |

Net retained bytes and peak extra bytes are identical in every case. Single
window controls are unchanged, as expected: there the old code collected
empty ranges and never allocated. A removes one allocation per inner window
edge, so two per window boundary (latin-split16: 179 boundaries, −358 calls). B only changes allocations where the origin cache is used (one map per
input instead of one per window).

DHAT (`loop <case> 10`, font loading included and identical across labels):

| case | base | ctx | prep |
| --- | ---: | ---: | ---: |
| latin-split16 | 8,970 blocks | 5,390 | 5,390 |
| arabic-split16 | 10,529 | 4,809 | 4,809 |
| cjk-upright-vorg-split16 | 13,367 | 5,207 | 5,207 |
| cjk-upright-novorg-split16 | 17,508 | 9,348 | 5,288 |
| ruby-base-split6 | 190,378 | 186,538 | 186,538 |
| latin-single | 3,710 | 3,710 | 3,710 |

## Instructions (callgrind, per build)

`(Ir(loop n=10) − Ir(loop n=0)) / 10`, release profile:

| case | base | ctx | prep | ctx vs base | prep vs ctx |
| --- | ---: | ---: | ---: | ---: | ---: |
| latin-single | 19,318,095 | 19,312,302 | 19,321,036 | −0.03% | +0.05% |
| latin-split16 | 20,742,240 | 20,688,581 | 20,451,196 | −0.26% | −1.15% |
| arabic-single | 10,401,009 | 10,398,697 | 10,404,547 | −0.02% | +0.06% |
| arabic-split16 | 12,165,386 | 12,089,073 | 11,901,740 | −0.63% | −1.55% |
| cjk-upright-vorg-single | 8,603,768 | 8,602,690 | 8,605,832 | −0.01% | +0.04% |
| cjk-upright-vorg-split16 | 10,612,714 | 10,526,701 | 10,189,576 | −0.81% | −3.20% |
| cjk-upright-novorg-single | 9,309,816 | 9,309,905 | 9,313,052 | 0.00% | +0.03% |
| cjk-upright-novorg-split16 | 50,491,436 | 50,409,541 | 10,888,561 | −0.16% | −78.4% |
| ruby-base-split6 | 47,579,616 | 47,531,845 | 47,385,360 | −0.10% | −0.31% |

The large `novorg-split16` drop comes from B's origin cache. Before B each
window started with an empty map, so every upright glyph ran
`cff_vertical_origin_delta` (a CFF outline bounds evaluation). After B each
distinct glyph is evaluated once per input. The single-window controls move
by under 0.1%.

## Wall clock

The run used one CPU (`taskset -c 10`), with labels interleaved in the order
base, ctx, prep, prep, ctx, base. Each cell is the median of 21 builds, one
value per run (ms):

| case | base | ctx | prep |
| --- | ---: | ---: | ---: |
| latin-split16 | 1.653 / 1.660 | 1.580 / 1.582 | 1.553 / 1.538 |
| arabic-split16 | 0.934 / 0.940 | 0.908 / 0.905 | 0.882 / 0.884 |
| cjk-upright-vorg-split16 | 0.759 / 0.758 | 0.723 / 0.716 | 0.693 / 0.696 |
| cjk-upright-novorg-split16 | 3.089 / 3.108 | 3.144 / 3.127 | 0.748 / 0.743 |
| ruby-base-split6 | 2.816 / 2.755 | 2.747 / 2.780 | 2.736 / 2.689 |
| latin-single | 1.332 / 1.308 | 1.226 / 1.238 | 1.264 / 1.225 |
| cjk-upright-novorg-single | 0.634 / 0.624 | 0.606 / 0.603 | 0.603 / 0.596 |

The single-window control `latin-single` moved about 7% between binaries
although its instruction count is unchanged. Wall-clock differences of a few
percent here are therefore within binary-layout and host noise, and no speed
claim is made for A from them. The only time result treated as real is the
upright CFF-without-VORG split case with B (about −76%), which matches the
callgrind drop. For the other split cases, the instruction counts give the
direction and size of the change: A −0.1 to −0.8%, B −0.3 to −3.2%.

## Decision

Both candidates are adopted. They do not change output or resource-limit
behavior.

- A removes per-window context allocations in multi-window inputs and has a
  small instruction effect. The motivation is allocation, not speed.
- B removes repeated face preparation and makes the CFF-without-VORG origin
  cache effective across windows. That path is the one material speedup:
  small budgets, ruby base scopes or very long upright CFF inputs no longer
  recompute outline bounds for every window.

## Reproduction

Data: shodo-zb0-7-shape-window-prep.tar.gz.
It contains the digest, alloc and time JSONL, callgrind and DHAT totals,
binary SHA-256s, the source revision of each label, and the scripts
(`snap.sh <label>`, `callgrind.sh base ctx prep`, `dhat.sh`, `time.sh`,
`cmp_alloc.py`). Labels: base a184341 (probe only), ctx 02258e0, prep daef4bf.
Toolchain rustc 1.96.0, valgrind 3.25.1, release profile.

## Verification

- `cargo test --locked --workspace` with `RUSTFLAGS=-D warnings`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `cargo test --locked -p shodo --lib missing_vorg` (fails with 10 ≠ 4
  evaluations when B is reverted)
