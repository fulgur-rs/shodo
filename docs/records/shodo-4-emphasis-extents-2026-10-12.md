# SHODO-4: auxiliary emphasis extents

`measure_unannotated` now exposes only `LineExtents` (baseline and block size).
Its shared solver omits the discarded record shift/membership vectors and
combination shift map. Full retained metrics still use the same solver with
shift output enabled. Public APIs and retained line storage are unchanged.

The solver still computes all contributing extents, including groups, ghosts,
root struts, and quirks. It revisits cached record/combination profiles after
group resolution to perform every discarded numeric conversion. The order
remains block size, over-side baseline, combinations, vertical-lr reflection,
then record shifts. Overflow/nonfinite counts and warning order are preserved.

## Measurement

- Baseline: `9e12da277bc5a241783c253d29552d92a42a7553`.
- Measured implementation: `38a93c7b83bc17a718936b5ac5830ff16808fec6`.
- Linux x86_64, AMD Ryzen 5 5600G, CPU 0 affinity, performance governor;
  Rust 1.97.1. Release debug=0, incremental=false; other profile defaults.
- Fixed Latin fixture SHA-256:
  `7aa5c6687e9a8b72f71ea5abaded28d771ecb54c66a8a9ac49268537b2f94d25`.
- Existing paragraph and warm context, first accepted line, consume block size
  then drop. Each of 8/512 text runs occupies its own inline box with baseline
  displacement -2/+3 px, giving 16/1024 fragments. Marked inputs use filled
  over-right dots; controls differ only in emphasis being disabled.
- Timing binaries have no allocator counters. Each small/long sample has
  2,000/200 iterations, with 20 warmups and five samples per process.
  Seven alternating baseline/fix pairs yield 35 samples per revision/case.
  All task builds finished before timing began.
- Separate allocation binaries use ten iterations, two warmups, three samples
  and three alternating pairs. All nine samples have identical allocation
  results; each scope ends with zero net bytes. Extra peak means requested
  live bytes above existing paragraph/context storage, rather than process RSS.

| Case | ns/line median, baseline → fix | Ratio | Allocated bytes/line | Calls/line | Extra peak bytes |
| --- | --- | --- | --- | --- | --- |
| Marked, 8 runs | 4,025.214 → 4,170.8745 | 1.0362 | 5,776 → 5,520 | 41 → 39 | 2,360 → 2,296 |
| Marked, 512 runs | 172,136.55 → 187,154.885 | 1.0872 | 387,184 → 370,800 | 1,085 → 1,083 | 147,008 → 142,912 |
| Control, 8 runs | 2,398.3585 → 2,346.115 | 0.9782 | 4,024 → 4,024 | 24 → 24 | 2,296 → 2,296 |
| Control, 512 runs | 90,602.065 → 92,238.175 | 1.0181 | 274,360 → 274,360 | 552 → 552 | 142,912 → 142,912 |

This is a temporary-memory improvement with a measured CPU cost. Marked
medians increased 3.6%/8.7%; controls changed -2.2%/+1.8%. Sample ranges overlap,
but the long marked median increase warrants explicit disclosure. The second
cached-profile pass trades recomputation for eliminating per-fragment storage.
These results do not establish a general speedup or unchanged latency.
The allocation benefit is adopted with this cost recorded: the long marked
case saves 16,384 requested bytes (4.2%) and 4,096 extra peak bytes (2.8%).
No claim is made about retained paragraph memory or combined-span allocation
counts, which this timing fixture does not contain.

The [JSON record](shodo-4-emphasis-extents-2026-10-12.json) retains all samples,
binary/source hashes, four matching output digests, mutation failure evidence,
and the exact measurement script. The 16 KiB traffic reduction comes from
4 KiB float shifts plus 12 KiB memberships for 1,024 fragments. Peak reduction
is smaller because their lifetimes overlap other transient allocations.

## Correctness and regression coverage

- Final workspace tests: 1,787 passed, zero failed, nine ignored. fmt,
  workspace/all-targets Clippy with warnings denied, and allocation-enabled
  Clippy for the new allocation test/timing example pass.
- Unit matrix compares full-output and extents-only solvers in 48 conditions:
  horizontal-tb/vertical-rl/vertical-lr × emphasis on/off × groups on/off ×
  text-combine on/off × ruby on/off. Baseline and block size match exactly.
- Another 36 cases inject positive/negative overflow, extreme finite values,
  NaN, and infinity into solver styles. Saturation counts and typed warning
  order match, including zero/one warning limits and pre-existing counts.
  Deleting discarded record conversion or combination conversion separately
  makes the test fail; both mutations were restored before final verification.
- The allocation regression fails on the baseline at 387,184 bytes against
  its 376,000-byte budget. Debug and release pass on the implementation.
- `emphasis_geometry` compares all 48 public-output conditions between the
  baseline and implementation: 441,978 JSON bytes match exactly, SHA-256
  `e132bf6b66e9f10840b1d64116dc5576d6096d7320b561c56af9de83b5389b56`.
  Output includes lossless glyph origins/baselines, extents, TCY transforms,
  ruby inner lines and recursive emphasis mark data. Fixture warnings are empty.
  The JSON record retains every case's digest. Existing literal emphasis
  geometry tests and CI glyph snapshots provide additional regression coverage.

## Reproduction

Use separate baseline/fix checkouts with identical new examples and their
support files. Set `TMPDIR="$HOME/tmp"`, `RUSTC_WRAPPER=""`, and separate
`CARGO_TARGET_DIR`s; unset `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER`.
Build each revision twice and save the binaries before changing features:

```sh
CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_INCREMENTAL=false \
  cargo +stable build --release -p shodo-bench --example emphasis_extents
cp "$CARGO_TARGET_DIR/release/examples/emphasis_extents" baseline-time
CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_INCREMENTAL=false \
  cargo +stable build --release -p shodo-bench --features allocation-counting \
  --example emphasis_extents
cp "$CARGO_TARGET_DIR/release/examples/emphasis_extents" baseline-alloc
taskset -c 0 ./baseline-time marked 512 200 20 5
taskset -c 0 ./baseline-alloc marked 512 10 2 3
```

Use `fix-time`/`fix-alloc` for the implementation. Repeat with `control` and
eight runs (2,000 timing iterations), alternating process order each pair.
The saved JSON script reproduces the full sampling schedule and checks.
For geometry, build/run `emphasis_geometry` on each revision, save its stdout,
and compare the files byte for byte. Run the workspace suite plus
`cargo test -p shodo-bench --features allocation-counting --test emphasis_extents_allocations`
in debug and release; the latter enforces the memory reduction independently
of wall-clock timing.
