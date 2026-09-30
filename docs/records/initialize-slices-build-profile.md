# `initialize_slices` build cost (2026-09-30)

Issue `shodo-xy2.13` revisits a profile that attributed 6.27% self time in
`arabic-long/1` build to `line::reshape::initialize_slices`. The current
baseline is main commit `a811247` with Rust 1.96.0 on this x86-64 machine.

Two `perf record -e cpu-clock -F 997 -g --call-graph dwarf` captures of the
existing `dev/bench` Arabic build benchmark, each with about 2,400 samples,
attributed 1.74% and 2.06% self time to `initialize_slices`. Unlike the earlier
build-only profile, this process profile includes `CheckedBuild::drop`'s
untimed validation, so the percentages are not directly comparable. Its
per-cluster path still searched
the entire ordered break-opportunity array twice and used a separate vector
to remember whether adjacent units shared storage.

The change advances one cursor through break opportunities as cluster ranges
increase. Overlapping cluster owners retain an exact binary-search fallback.
It also checks the preceding and following unit while consuming the original
unit list, avoiding the per-unit storage-split vector. Marker ordering,
`::first-line` cursor slices, and shape-window checks are unchanged.

## Release build benchmark

The existing `shodo-bench` `build/1` operation was measured in alternating
baseline/changed runs. Each run used 2 seconds warm-up, 100 samples, and a
5-second requested measurement period (Criterion extended sampling as needed).
Both worktrees used the same dependency build, fixed fixture fonts, workload,
compiler, and machine. Times are Criterion `slope.point_estimate` values in
microseconds, not sample medians or a cross-machine guarantee.

| Workload | Baseline | Changed | Difference |
| --- | ---: | ---: | ---: |
| `arabic-long/build/1`, pass 1 | 519.75 | 515.12 | -0.89% |
| `arabic-long/build/1`, pass 2 | 521.04 | 517.66 | -0.65% |
| `latin-long/build/1` | 496.35 | 486.14 | -2.06% |

The benchmark's complete operation-digest files match byte for byte between
baseline and changed runs for both workloads. One changed Arabic `perf` capture
attributed 1.40% self time to `initialize_slices`; the sample fractions are
only directional evidence because profiling and benchmark setup add work.
The total build improvement is modest; shaping and other phases dominate.

To repeat one timing run from the repository root:

```sh
SHODO_BENCH_CASE=arabic-long cargo bench -p shodo-bench --bench layout -- \
  'arabic-long/build/1' --warm-up-time 2 --measurement-time 5 --sample-size 100
```
