# SHODO-3: defer accepted-line block shifts

`Line::new` now leaves `block_shifts` empty, and edge-overlay materialization
does not maintain a copy. The only production constructor path applies
overlays, then immediately calls `measure_metrics`, which fills one shift per
final fragment before annotations, ruby placement, or any public line access.
The public API and completed-line invariant remain unchanged.

This removes temporary allocation and writes. It does not reduce the retained
paragraph data addressed by SHODO-1, and the ordinary-line timing medians did
not improve in this measurement.

## Conditions and results

- Baseline: `cffab0f62a05b79e27966cf8d4b66fcc804a5142`.
- Measured implementation: `6dc5bb6d93c315dacd620dcff17cba3b17f0fc20`.
- Linux x86_64, AMD Ryzen 5 5600G, CPU 0 affinity, performance governor;
  Rust 1.97.1. Release debug=0 and incremental=false; other profile defaults.
- Fixed Latin fixture SHA-256:
  `7aa5c6687e9a8b72f71ea5abaded28d771ecb54c66a8a9ac49268537b2f94d25`.
- Existing paragraph and warm context; accept the first line, consume its
  inline size, then drop it. Each text run is a separate styled inline with
  baseline displacement -2 or +3 px. Small/long cases have 8/512 runs and
  16/1024 ordinary fragments. Reshape appends `hy\u{ad}phenation` and fits
  only the prefix plus `hy-`, producing 17/1025 fragments and a real overlay.
- Timing uses a build without allocator counters: 2,000/200 iterations per
  small/long sample, 20 warmups, 5 samples per process, 7 alternating pairs
  (35 samples per revision/case). Final timing ran after task builds ended.
- Allocation uses a separate build: 10 iterations, 2 warmups, 3 samples per
  process, 3 alternating pairs. All 9 samples per revision/case have identical
  call/byte/peak results, and all scopes end with zero net bytes.
- Peak is extra allocator-visible requested live bytes during the scope,
  relative to existing paragraph/context storage. It is not process RSS.

| Case | ns/line median, baseline → fix | Time ratio | Allocated bytes/line | Calls/line | Extra peak bytes |
| --- | --- | --- | --- | --- | --- |
| Plain, 8 runs | 2,321.812 → 2,403.9145 | 1.0354 | 4,088 → 4,024 | 25 → 24 | 2,360 → 2,296 |
| Plain, 512 runs | 90,457.600 → 92,476.810 | 1.0223 | 278,456 → 274,360 | 553 → 552 | 147,008 → 142,912 |
| Reshape, 8 runs | 3,807.3735 → 3,695.203 | 0.9705 | 15,303 → 14,995 | 81 → 76 | 5,868 → 5,672 |
| Reshape, 512 runs | 117,284.595 → 115,102.295 | 0.9814 | 906,567 → 886,099 | 1,635 → 1,624 | 316,332 → 304,040 |

Ordinary medians increased 3.5%/2.2%; reshape medians decreased 2.9%/1.9%.
Sample ranges overlap in all cases. These timings do not establish a general
speedup or unchanged latency. The change is adopted for the verified temporary
memory reduction, with the small ordinary-case timing increases disclosed.
An initial series collected during parallel builds had much wider ranges
(including +27.3% for the long plain median); it is preserved separately as
`exploratory_timing`, not silently discarded or pooled with the final series.

The [JSON record](shodo-3-block-shifts-2026-10-12.json) includes every sample,
output digest, executable/source hash, and both series. All four fixed-input
output digests match across both revisions and measurement builds.

## Initial zeros and reshape copies

A separate diagnostic baseline printed `records.len() * size_of::<LayoutUnit>()`
at construction and `shifts.len()`/`shifts.capacity()` before overlay replacement,
and asserted every copied value was zero. Its patch and traces are in the JSON;
it was never used for timing or allocation measurements.

| Case | Initial zero bytes removed | Logical reshape-copy bytes removed | Reshape scratch capacity removed |
| --- | --- | --- | --- |
| Plain, 8 runs | 64 | 0 | 0 |
| Plain, 512 runs | 4,096 | 0 | 0 |
| Reshape, 8 runs | 68 | 68 | 128 |
| Reshape, 512 runs | 4,100 | 4,100 | 8,192 |

These are logical writes, excluding allocator-internal realloc copies or
machine instruction counts. Growth requests for the reshape scratch add
240/16,368 bytes of allocation traffic, so the measured 308/20,468-byte savings
equal initial zeros plus the scratch growth requests. Peak savings are
196/12,292 bytes, equal to initial storage plus final scratch capacity.
The release allocator results demonstrate that the old allocations survived
compiler optimization for these inputs. Final metrics allocation is preserved.

## Correctness and verification

- Baseline `cargo +stable test -p shodo`: 1,251 passed, 0 failed, 9 ignored.
- Final `cargo +stable test --workspace`: 1,785 passed, 0 failed, 9 ignored.
- New completed-line tests check empty/2-run/512-run lines, real SHY overlays,
  fragment/shift cardinality, literal -2/+3 baseline shifts, glyph origins,
  line height, narrow overflow, and saturation/nonfinite warning contents/order.
- Allocation regression fails on baseline at 278,456 bytes against the
  276,000-byte plain budget; debug and release pass on the implementation.
  A separate 895,000-byte reshape budget detects scratch-copy allocation.
- Existing `soft_hyphen_diff` on 256 deterministic mixed-script/multi-run cases:
  11,234 lines with default limits and 10,440 with reshape-window budget zero.
  Lossless output/intrinsic and ordered warning hashes match in both revisions.
- fmt, diff checks, workspace/all-targets Clippy with warnings denied, and
  allocation-enabled Clippy on the new test/example all pass.

The runtime change is confined to unused pre-metric buffers. Reusing the
initial vector instead would retain its zeros and complicate metrics ownership;
adding a separate unfinished-line type would expand this bounded change.
If a future caller adds pre-metric consumers, it must preserve the explicit
construction → overlay → metrics ordering contract.

## Reproduction

Use isolated checkouts for the two revisions, with the same new example and
`support/block_shift_input.rs` copied into the baseline checkout. Do not copy
the diagnostic patch into measurement builds. Use separate target directories:

```sh
mkdir -p "$HOME/tmp/shodo3-repro"
TMPDIR="$HOME/tmp" RUSTC_WRAPPER="" \
  CARGO_TARGET_DIR="$HOME/tmp/shodo3-repro/target" \
  CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_INCREMENTAL=false \
  cargo +stable build --release -p shodo-bench --example block_shift_materialization
# Save this executable before enabling allocator counting.
taskset -c 0 "$HOME/tmp/shodo3-repro/target/release/examples/block_shift_materialization" plain 8 2000 20 5
taskset -c 0 "$HOME/tmp/shodo3-repro/target/release/examples/block_shift_materialization" reshape 512 200 20 5
```

Repeat for both cases and run counts, alternating revision order for 7 pairs.
Build with `--features allocation-counting` for the separate allocation runs
(`10 2 3`, 3 pairs). Run `cargo +stable test -p shodo-bench --features
allocation-counting --test block_shift_allocations`, also with `--release`.
For differential output, build `--example soft_hyphen_diff` in each revision,
run it with `256`, then with `SOFT_HYPHEN_DIFF_WINDOW_BYTES=0`.
The existing `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER` was unset for builds
and tests; measurement executables were invoked directly.
