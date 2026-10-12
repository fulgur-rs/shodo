# SHODO-16: guarded first-line analysis and shaping reuse

The duplicate build is avoidable when first-line changes only paint. This
evaluation retains separate normal/alternate paragraph data and both
finalizers, while copying pure analysis and selected clean shape output.
SHODO-13 / PR #300 remains held by the user; this work starts independently
at `c64b8b3c023ee6c527299ece83967bdf3c1011b2`.

## Scope and exact effects

Every paired sanitized style must match outside paint, with exact inherited
float/variation bits. Ruby scopes, external break callbacks, dirty/suppressed
warnings, numeric saturation, and insufficient text/item/glyph bounds retain
the original path. The analysis bound also covers pre-transform whitespace
items, so shrinking transformations cannot bypass an intermediate error.
Style/resource accounting continues to charge both retained datasets.

Selected automatic combined-width features with finite glyph limits use full
alternate shaping. A real GSUB fixture expands `aa` to four plain-trial glyphs
then contracts it to two selected glyphs. With aggregate cap four, the old
alternate fails at `ShapedGlyphs { limit: 4, actual: 6 }`; bounding only copied
glyphs incorrectly succeeded. That regression was observed RED, the guard
made it GREEN, and caps 0..11/unlimited are compared against the reference.
Unlimited combined-width reuse remains covered by actual two-versus-four
Harfrust calls. Analysis can still be reused under finite combined-width caps.

Font-size/transform changes retain complete alternate analysis and shaping.
Build has no maximum later line width, so a safe first-line prefix cannot be
inferred. This change does not accelerate that class or remove alternate
retained paragraph data.

## Reproduction and controls

The checked-in measurement is
[`first_line_build_reuse.rs`](../../dev/bench/examples/first_line_build_reuse.rs).
The companion [JSON record](shodo-16-first-line-reuse-2026-10-12.json) retains
the exact baseline/candidate benchmark sources, snapshot helper, driver,
toolchain, binary hashes, all process/sample times, allocations and output
digests. Only whitespace formatting differs between the archived baseline
measurement source and checked-in candidate measurement source.

Machine: AMD Ryzen 5 5600G, 6 cores/12 threads, Linux x86_64, boost enabled.
Counter-free timing processes use CPU 2, five alternating baseline/candidate
pairs, seven samples per case per process. Both independent release builds and
verification builds finish before timing. Allocation counting runs separately
with three samples per case; its times are excluded from speed comparisons.
Release profile overrides: `debug=0`, `incremental=false`, Rust stable 1.97.1,
`RUSTC_WRAPPER=""`, `TMPDIR="$HOME/tmp"`.

Use separate Cargo target directories for the two worktrees. An initial
shared-target build returned byte-identical baseline/candidate binaries without
compiling the candidate; all those smoke timings were discarded. A later
timing start overlapping final debug verification was also discarded.

Builder/source creation, context creation, workload construction, hashing,
oracle traversal and result release are outside the operation windows.
`analyze`, `shape` and direct `build` have separate windows. Collection font and
shaper caches are warmed before sampling; scratch contexts are fresh.
Analysis consumes a recorded builder, shape consumes its analysis, and build
consumes a separately recorded builder. Allocation net/peak therefore describe
each operation window and include release of consumed input; they are not
whole-process RSS or total retained memory from a zero-input start.

For each of 8/4000 words, cases are none, same, paint, decoration, size,
transform, CJK, RTL, vertical, combined, structural and Ruby. The complete
recursive oracle visits every accepted line at widths 0, 32, 240 and f32::MAX,
with Auto/Balance/Pretty plans and a height rejection for every line. It
includes glyph/source/cluster fields, metrics, paint/decorations, first/later
line styles, mappings, intrinsic widths, ordered warnings, opaque-token unit
and flag fields, and recursive Ruby geometry. Only paragraph/layer ownership
identity is normalized by the pinned helper. Full text/mapping values are
interned losslessly and hashed with their reference table. The complete
`analyze+shape` and direct-build oracles also agree within each revision.
All timing/allocation samples additionally check the same public text/mapping/
build-warning summary against their revision's complete oracle run.

## Measurements and adoption

The final runtime matrix uses `b00fccfa6e0253ddabcaa55eed8e0bf0d66d5e10`.
Each phase median pools 35 counter-free samples. Negative change is faster;
phase medians are reported separately and are not summed into an end-to-end
median. All 24 recursive outputs agree: 1,388,244 oracle entries per revision.

| Case | Words | Baseline build ms | Candidate build ms | Build change | Analyze change | Shape change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| none | 8 | 0.0491 | 0.0509 | +3.7% | +3.7% | +3.5% |
| same | 8 | 0.0829 | 0.0653 | -21.2% | -23.4% | -19.7% |
| paint | 8 | 0.0794 | 0.0624 | -21.4% | -23.6% | -20.8% |
| decoration | 8 | 0.0791 | 0.0644 | -18.6% | -23.9% | -19.9% |
| size | 8 | 0.0803 | 0.0894 | +11.4% | +3.9% | +4.5% |
| transform | 8 | 0.1318 | 0.1402 | +6.4% | +3.3% | +3.5% |
| cjk | 8 | 0.0577 | 0.0469 | -18.8% | -23.3% | -15.3% |
| rtl | 8 | 0.1108 | 0.0816 | -26.3% | -26.9% | -21.3% |
| vertical | 8 | 0.0833 | 0.0644 | -22.7% | -22.8% | -22.1% |
| combined | 8 | 0.1182 | 0.1170 | -1.1% | -21.4% | +2.5% |
| structural | 8 | 0.1087 | 0.0858 | -21.1% | -23.1% | -18.1% |
| ruby | 8 | 0.1166 | 0.1269 | +8.8% | +5.2% | +4.1% |
| none | 4000 | 15.5113 | 16.0318 | +3.4% | -0.1% | +3.2% |
| same | 4000 | 33.8078 | 26.8405 | -20.6% | -44.9% | -17.4% |
| paint | 4000 | 33.9725 | 26.7212 | -21.3% | -43.0% | -18.5% |
| decoration | 4000 | 33.5499 | 26.7648 | -20.2% | -42.8% | -8.1% |
| size | 4000 | 32.2038 | 33.9394 | +5.4% | -0.1% | +13.7% |
| transform | 4000 | 53.8216 | 55.1200 | +2.4% | -0.7% | +2.9% |
| cjk | 4000 | 18.7301 | 14.7519 | -21.2% | -42.9% | -14.7% |
| rtl | 4000 | 42.1825 | 32.4498 | -23.1% | -41.8% | -20.8% |
| vertical | 4000 | 30.9038 | 22.3799 | -27.6% | -42.8% | -25.2% |
| combined | 4000 | 31.7257 | 30.0516 | -5.3% | -42.6% | +1.4% |
| structural | 4000 | 31.5592 | 23.5620 | -25.3% | -43.0% | -19.4% |
| ruby | 4000 | 20.2957 | 20.3042 | +0.0% | +1.1% | -0.2% |

For long paint, direct-build allocation calls fall 735 -> 523 and allocated
bytes 34,798,185 -> 29,752,450. Window net bytes fall 15,004,174 -> 14,495,216
and peak extra bytes 15,656,750 -> 15,147,792. Short paint calls fall
357 -> 280, allocated bytes 85,846 -> 74,761, net 41,419 -> 39,613 and
peak extra 42,187 -> 40,381. Copies retain both alternate datasets; this is
less repeated work, not elimination of their memory cost.

The first complete matrix at `2378eec` is preserved in
`prior_guard_evaluation`, including long-size shape +17.7%. Early float
rejection removes two allocation calls and 43 bytes on that analysis fallback;
it does not establish a cause for the remaining +13.7% shape regression.
Size-control shape allocations remain exactly 595 calls / 31,708,690 bytes
in both revisions. Transform fallback still has the guard's two extra calls /
43 bytes, with unchanged net/peak bytes. Neither matrix is dropped when a
later diagnostic produces different results.

An isolated diagnostic runs only one long case in each fresh process, with
seven alternating pairs / 21 samples per case (147 per revision/case), using
the same runtime revisions and separate release targets. Size shape changes
24.7595 -> 24.1825 ms (-2.3%), build 32.8451 -> 31.8996 ms (-2.9%);
none build changes -15.7%, paint build -23.2%. The isolated source adds a
case-filter branch, so process allocation history and compiled benchmark code
layout both differ. This does not identify the mixed-matrix regression's
cause or prove it is harmless. Its exact source, binary hashes, raw samples
and one userspace perf-stat diagnostic are preserved in
`isolated_fallback_probe`. Every isolated sample matches the complete-matrix
public summary; the probe binaries have no separate complete recursive oracle.
The primary adoption evidence remains the fully verified mixed matrix.

Adopt the guarded path for the measured paint-only class: both valid mixed
matrices consistently reduce its build time by about 20%, real work counters
remove the repeated passes, requested allocation traffic falls, and exact
output/limit behavior is covered. Accept the recorded fallback/control
slowdowns as a scoped tradeoff, including long-size split shape +13.7% and
short-size build +11.4% in the final matrix. This is not a claim of universal
speedup or regression-free fallback. The cause of the timing sensitivity is
unresolved; the redundant style clone was independently reproduced and fixed,
but is not offered as its explanation.

To reproduce, create separate worktrees at the JSON `baseline` and
`runtime_candidate` revisions, restore `compiled_baseline_measurement_source`
and `source` respectively as `dev/bench/examples/first_line_build_reuse.rs`,
and restore/compare `snapshot_source`. Build each counter-free example with
`cargo +stable build --release -p shodo-bench --example first_line_build_reuse`
in distinct target directories and copy the resulting binaries as
`baseline`/`candidate` under a fresh `~/tmp` directory. Repeat with
`--features allocation-counting`, copying as `baseline-allocation` and
`candidate-allocation`. Finish all builds, then run
`taskset -c 2 <binary> 1 oracle` for both revisions into the corresponding
`*-oracle.json`. Save the archived prior matrix as `prior-guard-evaluation.json`
there and run the archived `driver_source` from the candidate worktree with
that scratch path and a fresh output path. The isolated probe is reproduced
from its archived source as `first_line_fallback_probe.rs`, using
`SHODO_PROBE_KIND=size|none|paint taskset -c 2 <probe> 21 timing`, alternating
revision order for seven pairs. All preparation and hashing stay outside
measurement windows.

## Verification

Nine focused tests compare the actual optimized/reference paths. The analysis
work regression was two passes versus one. After analysis reuse, the real-font
matching/shaping regression was `(analysis, matching, Harfrust) = (1,52,2)`
versus the normal build's `(1,26,1)`; selected shape reuse matches `(1,26,1)`.
These counters observe production work and are test-only. A separate counter
observed one unnecessary owned-style clone on the changed-size fallback;
checking inherited float bits first reduces it to zero without changing the
eligibility set. That test was observed RED before the check was reordered.

Coverage includes transformed and resolved style maps, cross-node ligatures
with distinct paint owners, warm contexts, first/later paint, malformed paint,
NaN/numeric outliers, zero/one warning caps, callback invocation, recursive
Ruby, signed-zero variation values, exact text/item caps 0..119, glyph caps
0..89, and combined-width trial caps 0..11/unlimited. All ordinary data
finalizers and source/cursor mapping remain in use.

The final runtime commit passed 1,805 workspace tests, with zero failures and
nine ignored tests. Both ordinary and accesskit Clippy checks pass with
warnings denied. Independent review and exact-head CI results are pending.

## Rulings and their costs

1. Keep full shape-changing alternate data because build cannot bound a later
   first-line width. Cost if wrong: size/transform retains its duplicate cost.
2. Keep separate datasets and both finalizers to preserve paint/source/cursor
   semantics. Cost if wrong: copied vectors and alternate storage limit gains.
3. Bound pre-transform items too because retained items alone do not prove
   whitespace success. Cost if wrong: tight valid budgets can miss reuse.
4. Derive private TransformSpan Clone to copy Processed faithfully. Cost if
   wrong: copied source-span storage adds allocation traffic.
5. Isolate release target directories after detecting stale shared artifacts.
   Cost if wrong: extra dependency build time and scratch disk use.
6. Run complete oracles separately from timing. Cost if wrong: timed samples
   have public-summary checks; complete geometry is checked in a dedicated run.
7. Intern repeated full text/mappings losslessly. Cost if wrong: correctness
   depends on the archived interning code and recursive reference table.
8. Use full alternate shaping for selected automatic width features under
   finite glyph caps. Cost if wrong: even ample finite caps miss those shaping
   gains, while analysis reuse remains.
9. Adopt the scoped paint-only gains while preserving both mixed matrices and
   the isolated diagnostic, without assigning an unproved cause to timing
   sensitivity. Cost if wrong: fallback/control-heavy callers may incur the
   measured +13.7% split-shape / +11.4% short-build slowdowns or larger
   unmeasured regressions, despite benefits for the intended class.

## Limits of the evidence

The fixtures, font/cache warmth and machine define this measurement. It does
not establish performance for every system font, cold font collection, OS/CPU,
large style set or caller. Retained alternate data and finalization costs
remain. Finite-cap selected combined-width cases retain full shaping, and
Ruby/shape-changing/callback/dirty-numeric paths retain their full work.
