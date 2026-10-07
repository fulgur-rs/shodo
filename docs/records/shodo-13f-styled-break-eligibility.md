# shodo-13f: reuse styled-break eligibility for growing ranges

Measured on 2026-10-07 from released 0.0.21, base
`3cc51c0b3572c573a4d92fcab3376afbce52d936`.
Follow-up to [PR256's performance observation](https://github.com/fulgur-rs/shodo/pull/256#discussion_r4203160019).
The complete comparison, five timing samples per build and workload, allocation
scope counters, and output hashes are in
`shodo-13f-styled-break-eligibility.summary.json`.

## Reachable workload and regression

Ordinary scanning ends at the first forced break. Public
`LineConstraint.max_graphemes` deliberately ignores forced boundaries. When a
ruby base contains many forced breaks, count-mode scanning asks the metric index
about growing prefixes while considering ruby candidates. This is a reachable
public workload; a plain paragraph without ruby is a necessary control because
its count-mode retained emission does not have the same repeated scalar queries.

The previous selector walked all earlier forced breaks and repeated parent
content-credit queries for each styled break. The new public regression failed
before the runtime change with the following dense eligibility-decision counts:

| Breaks | Before | After | Scalar calls, unchanged |
| --- | ---: | ---: | ---: |
| 64 | 4,290 | 130 | 99 |
| 128 | 16,770 | 258 | 195 |
| 256 | 66,306 | 514 | 387 |

The sparse case, with one styled break and otherwise plain breaks, changed from
258/514/1,026 decisions to 130/258/514. Dense indexed visits changed from
140,110/527,504/2,095,546 to 34,556/75,804/165,568. The test observes the public
`next_line` ruby path, checks that scalar queries actually occur, and guards
subquadratic decision-count growth rather than relying on elapsed time.

## Implementation and bounds

The conditional break index stores styled profiles and consecutive parent runs.
For a fixed range start, it prepares each newly selected profile once for each
of two whitespace modes, then joins cached summaries. Selection splits at the
current trailing-whitespace boundary. Before that boundary it uses full content
credit; the trailing suffix combines preserved-content credit with a shared
prefix-credit query per parent run. A trailing run contains no Open unit, so its
parent changes follow closing ancestry rather than every break.

Eligibility preparation for growing prefixes totals
`O(S * (log U + log S))`, where S is selected styled breaks and U is units.
Ordinary summary queries take logarithmic work plus trailing-parent queries.
Plain breaks do not enter this index. A query selecting zero or one styled break
does not allocate prepared summaries. The cold index construction does allocate
two compact metadata vectors instead of one dense conditional summary tree.

The cache retains only one range start. Changing that start resets its prepared
window; clipped queries cannot include stale leaves. Arbitrarily alternating
starts can repeat preparation. Conditional group traversal and affected-group
placement are still required, so this change does **not** promise linear total
`MetricIndex::select` work for every many-group workload. Profile metadata and
prepared trees are O(S); the existing position-only ghost tree remains O(U).

Height, baseline, selected group bounds and ghost placement continue to use each
break's local parent-credit decision. Chromium semantics recorded for shodo-r8t
are preserved, including top-aligned atomic content, pending alignment barriers,
and continuation edges.

## Measurement conditions

AMD Ryzen 5 5600G, Linux x86_64, CPU 0 via `taskset`, Rust
`1.96.0 (ac68faa20 2026-05-25)`. Both builds use release opt-level 3, LTO off,
16 codegen units, and default features. Timing builds have no counting allocator;
separate `allocation-counting` builds measure two layouts per scope.

The committed `styled_break_scale` example supplies fixed Shodo Fixture CJK
fonts, 10px parent styles, 40px own break styles, and alternating parents with
one CJK character or no text. Ruby reading uses the same fixed-font character.
There are 72 workloads: plain/ruby × control/sparse/dense × normal/count ×
64/128/256 × cold/warm. Width is 96px; count mode uses `usize::MAX`.

Font loading, paragraph construction, context creation, warm-up, output and
warning hashing are outside timing and allocation scopes. Layout and line-vector
drops are included; cold contexts also drop inside the scope. Warm mode reuses
one context. Cold context creation is intentionally outside the scope, and its
drop explains the negative net-byte delta in cold allocation counters.

An initial three-sample, eight-layout measurement showed large time variations
even in unchanged plain controls. Its timings were superseded by five paired
samples of sixteen layouts, alternating baseline/candidate order on successive
samples. No task compiler jobs ran during either timing batch. External scheduling
and frequency variation remain: samples and control results are retained rather
than treating every median difference as a proven regression or improvement.

| Dense ruby count mode | Before, ms/layout | After, ms/layout | After/before |
| --- | ---: | ---: | ---: |
| 64, cold | 1.857 | 1.010 | 0.544 |
| 64, warm | 1.472 | 0.614 | 0.417 |
| 128, cold | 5.994 | 2.204 | 0.368 |
| 128, warm | 5.061 | 1.348 | 0.266 |
| 256, cold | 23.252 | 4.627 | 0.199 |
| 256, warm | 25.126 | 2.807 | 0.112 |

At 256 breaks, plain controls have median ratios 0.992–1.045. Ruby control
ratios span 0.965–1.259 and include overlapping, noisy timing samples; no blanket
speed claim is made for ordinary or unstyled ruby layout. Dense ordinary ruby
ratios are 1.023 cold / 1.009 warm with unchanged warm allocation counts.

Dense count-mode cold allocation calls/layout fell from 19,323 to 13,848 and
allocated bytes/layout from 7,717,271 to 6,992,199. Warm calls fell from 16,980 to
11,490 and bytes from 2,276,373 to 1,629,189. Retained live bytes fell by 94,016.
Sparse ordinary cold layout adds one metadata allocation but saves 147,008
allocated bytes/layout; dense ordinary cold adds thirteen small metadata-growth
allocations but saves 114,752 bytes/layout. Unstyled ruby controls have unchanged
allocation calls and a bounded 192-byte cold/retained structure-size increase.
Plain control allocation counters are unchanged.

All 72 public full output hashes and warning hashes match baseline. Snapshots
cover glyph identifiers, logical/source mappings, positions, baselines, fragments
and ruby subtree geometry. Build and layout warnings are empty in all workloads.

## Reproduction

Use separate checkouts at the base above and this change. Copy the committed
`dev/bench/examples/styled_break_scale.rs` into the base checkout and append its
`[[example]]` entry from `dev/bench/Cargo.toml`; this adds only the measurement
driver to the old runtime. The snapshot helper already exists in the base.
Put temporary checkouts, binaries and target directories below `~/tmp`.

```sh
TMPDIR="$HOME/tmp" CARGO_TARGET_DIR="$HOME/tmp/shodo-13f-timing-target" \
  CARGO_PROFILE_RELEASE_OPT_LEVEL=3 CARGO_PROFILE_RELEASE_LTO=false \
  CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
  cargo build -p shodo-bench --release --example styled_break_scale
# Preserve each build's executable before building the other revision.
taskset -c 0 ~/tmp/shodo-13f-baseline-timing ruby dense count 256 cold 16
taskset -c 0 ~/tmp/shodo-13f-candidate-timing ruby dense count 256 cold 16
```

Repeat five times per workload, reversing build order on odd samples. Change
arguments using the full Cartesian product listed above, and compare the median
`elapsed_ns / reps`. Build again in a separate target directory with
`--features allocation-counting`; run two repetitions for allocations, never use
that build's elapsed time. Compare `output_sha256` and `warnings_sha256` across
both builds for every workload. The example asserts valid arguments and hashes
outside the measured scope.

```sh
cargo test -p shodo --lib styled_break -- --nocapture
```

The focused suite includes retained height/baseline/content comparisons, all
three writing modes, collapsed/preserved spaces, inline edges, adjacent breaks
sharing a parent, pending alignment, full/partial Top/Bottom groups, suppressed
ghost placement, cache start changes, shrinking windows, and ordinary-prefix
scaling. Independent read-only review of the code/test/driver patch
`e6966856691c8c84ed1d9088ac68ebaae2cdefef05a7cf7d69c71ac71c417661`
found no Critical, Important or Minor issues. Local verification passed:

- `cargo test --workspace`: 1,704 passed, 9 pre-existing ignored tests.
- `cargo fmt --all --check` and `git diff --check`.
- `cargo clippy --workspace --all-targets -- -D warnings`, also with
  `--features shodo-harness/accesskit`.
- `cargo test -p shodo --no-default-features`, also with `--features complex-scripts`.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`.
- `cargo test -p shodo-bench --features allocation-counting --test allocator --test probe`.

The development test builds use opt-level 1 and line-table debug information.
MSRV 1.89.0, Wasm and the complete repository CI matrix run on the pull request.
