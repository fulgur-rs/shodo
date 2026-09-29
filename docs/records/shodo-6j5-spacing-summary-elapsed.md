# shodo-6j5: spacing summary memoization elapsed time

Measured on 2026-09-30 JST. The complete per-operation samples, output
digests, line ranges/reasons, source and binary hashes, and environment are in
[`shodo-6j5-raw.json`](shodo-6j5-raw.json). The measurement program is
[`spacing_summary_elapsed.rs`](../../dev/bench/examples/spacing_summary_elapsed.rs).

## Compared revisions and operation

- Base: `c4e34bd9216464b52d85c46c2bee463121a80ae0`.
- Memoized: `edaebb3ac0c23b98cc0a061d0b4a2ad46df0beee`.
- The diff between those revisions changes only `src/line/spacing.rs` and
  `src/line/spacing_summary.rs`. The same measurement source (SHA-256
  `78d2a866d947c92351924d57190f5ef9b3242c60269d943dc8dc35c2eb9ba7fb`)
  was copied into both source trees.
- `build` times `Workload::build` with an already loaded, fixed font collection
  and a reused `LayoutContext`. `break_all` times `Paragraph::break_all` on a
  built paragraph at the fixture width with default line options. A sample
  ends before `black_box` and output destruction. Digest and line-semantic
  inspection happen outside the timed interval. These are warm operation
  timings, not initialization or end-to-end browser timings.
- Both revisions used `cargo build --offline --release`, Rust 1.96.0, the
  same resolved Cargo.lock (SHA-256
  `2b18280c59bba45cafc0be0d077a9aaee8bf1814c4639675312c23204f81fdc5`),
  and **separate empty target directories**. An attempted shared target
  directory incorrectly reused the base binary and was discarded. The
  accepted binaries have distinct SHA-256 values in the raw record.

Each case ran 30 warmup operations followed by 100 individually timed samples
per pass, in base/changed/changed/base order. Thus each revision has 200 raw
samples per operation and case. The table shows pooled medians in microseconds;
the parenthesized range is the 25th–75th percentile. A lower ratio is faster.

| Fixed input | Build base → changed | Build ratio | `break_all` base → changed | `break_all` ratio |
| --- | ---: | ---: | ---: | ---: |
| Arabic long, 1379 bytes | 566.9 (559.0–572.9) → 565.0 (555.9–572.9) | 0.997× | 484.5 (478.9–495.5) → 446.4 (440.8–453.9) | 0.922× |
| Latin long, 959 bytes | 559.9 (550.2–565.0) → 562.2 (553.4–567.5) | 1.004× | 357.7 (353.3–368.6) → 359.6 (356.3–368.4) | 1.005× |
| Japanese long, 1548 bytes | 784.8 (778.4–790.6) → 773.2 (767.2–793.5) | 0.985× | 258.9 (257.7–267.4) → 260.0 (257.0–266.7) | 1.004× |
| Mixed scripts ×8, 528 bytes | 355.9 (349.5–367.2) → 344.1 (340.4–354.0) | 0.967× | 175.0 (173.4–219.0) → 166.6 (165.3–168.3) | 0.952× |

The Arabic `break_all` difference is present in both pass pairs: base medians
487.9/483.6 µs and changed medians 441.7/449.7 µs. Its observed median is
7.8% lower. Latin and Japanese `break_all` differences are about 0.5% and
their interquartile ranges overlap. The mixed case favors the changed revision
in these passes, but the base pass medians vary from 174.5 to 189.5 µs; this
does not support a stable percentage claim. Build differences are small or
overlap their interquartile ranges. No elapsed threshold was added to tests.

For each input, all four passes produced identical line ranges and break
reasons, plus identical digests covering glyph IDs, clusters, advances,
positions, and other rendered fragments. The font bytes, fixture corpus,
settings, and output hashes are recorded in the raw JSON. These cases are
ordinary fixture paragraphs; they do not reproduce the earlier 127-level
Cursor microcase. Its zero frame visits alone do not imply a matching whole
layout speedup.

## Reproduction

Use the two commits above as separate worktrees and copy the measurement
program to `dev/bench/examples/spacing_summary_elapsed.rs` in each. Build
each in an **independent empty** `CARGO_TARGET_DIR`:

```sh
CARGO_TARGET_DIR=/tmp/shodo-6j5-base-target cargo build --offline --release -p shodo-bench --example spacing_summary_elapsed
CARGO_TARGET_DIR=/tmp/shodo-6j5-changed-target cargo build --offline --release -p shodo-bench --example spacing_summary_elapsed
```

From the respective worktrees, run each built binary as
`spacing_summary_elapsed CASE SCALE 30 100`. The cases are `arabic-long 1`,
`latin-long 1`, `japanese-long 1`, and `mixed-scripts 8`. Run each case in
base/changed/changed/base order; each invocation prints JSON with the raw
nanosecond samples and output. Compare `digest` and `line_semantics` exactly
before aggregating. The raw record retains all four passes and pooled
median/quartiles. No raikiri spike or WPT baseline was changed.
