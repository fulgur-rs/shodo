# SHODO-2: descriptor presence without owned clones

This record measures removal of the `face_descriptor(id).is_some()` clone in `selected_match`. The private bool lookup borrows the slot under the existing catalog mutex and follows the same parent lookup. The public owned-returning API, selection/ranking, catalog updates and lock scope stay unchanged.

Baseline: `732d61aa568558d1343842b8980961063be3b550`. The accompanying [JSON](shodo-2-descriptor-2026-10-12.json) pins the measured fix commit and source hashes and retains all observations.

## Conditions

- AMD Ryzen 5 5600G, Linux, CPU 0 affinity, performance governor; Rust stable 1.97.1. This is a local observation, not a portable performance guarantee.
- Release defaults with debug info and incremental compilation disabled; no allocation counting in timing binaries. Allocation counts come from separately built `allocation-counting` binaries.
- Identical `font_descriptor_lookup.rs` source on both revisions: SHA-256 `d57e7213d6d8688954bcf62247d2983e1ed977e82de8cd239f2c93ca0a8ed1d5`.
- Fixed Latin font SHA-256 `7aa5c6687e9a8b72f71ea5abaded28d771ecb54c66a8a9ac49268537b2f94d25`; no system fonts.
- Registration, query creation, candidate/cache warmup and report allocation are outside timed/counted work. Every match consumes its result and verifies empty variations/no synthesis. Selection index and digests agree between revisions in every run.
- Timing: 20,000 matches per sample, 2 warmups, 5 samples per process, 7 alternating baseline/fix pairs (35 samples per revision/case). Counts: 1,000 matches, 2 warmups, 3 samples per process, 3 alternating pairs. All raw observations are retained in JSON.
- Ranges deliberately repeat `(0, 0x10ffff)` to vary a valid descriptor's owned payload independently of coverage. These are synthetic clone-cost inputs, not a typical font catalog distribution.

## Results

| Case | Faces / ranges / match cache | Median ms baseline → fix | Fix / baseline | Allocation calls per 1,000 baseline → fix | Requested bytes per 1,000 baseline → fix |
| --- | --- | --- | --- | --- | --- |
| prepared-small | 1 / 0 / 1 | 8.111 → 7.822 | 0.964 | 11,000 → 10,000 | 305,000 → 295,000 |
| prepared-heavy | 16 / 128 / 1 | 43.581 → 43.909 | 1.008 | 12,000 → 10,000 | 1,329,000 → 295,000 |
| raw-static | 1 / 128 / 0 | 8.542 → 8.066 | 0.944 | 13,000 → 11,000 | 3,172,000 → 2,138,000 |
| cache-hit | 16 / 128 / 256 | 1.366 → 1.364 | 0.998 | 2,000 → 2,000 | 34,000 → 34,000 |

Prepared and raw cases alternate `a`/`b`; cache capacity one forces result-cache misses but retains prepared family/property candidates. `raw-static` disables candidate and match caches. `cache-hit` repeats `a` after warmup and bypasses selection.

Release counts prove these copies were not eliminated by the compiler: one 10-byte family allocation disappears per small selected match, and a 1,024-byte range allocation disappears as well in the heavy/raw cases. All allocation samples in each case/revision agree. Net allocation is zero; retained catalog/candidate bytes do not change.

Small prepared and raw matches have lower observed medians. The descriptor-heavy case does **not** establish a speed improvement: its median is 0.8% higher and the ranges overlap (42.985–45.034 ms baseline, 42.267–45.539 ms fix). The cache-hit control has identical counts and effectively identical timing. Do not infer whole-paragraph or document speed gains from these measurements.

## Design decision and validation

Keep the borrowed bool lookup rather than adding presence metadata to every Candidate/native-materialization/prepared-cache path. It achieves the requested allocation removal and preserves catalog consistency without new retained state. A candidate flag could still remove mutex overhead, but this measurement does not isolate a need for that additional change. If that choice proves limiting, the cost is a future performance optimization, not different font selection.

The regression deliberately warms a static face with 1,024 ranges and then performs eight alternating public matches. Before the change it allocates 67,976 bytes in 96 calls and fails the `< 8,192` byte budget; after the change it passes. This tests the consumer-visible allocation behavior rather than counting calls to a clone helper.

Layer visibility tests cover the stub, raw registration, shared/document/sibling/foreign IDs, an invalid index, later shared registration, and generation invalidation on document registration. Slant tests at cache capacities 0/1 check descriptor-controlled `slnt=-10` against descriptor-less request-controlled `slnt=-15`, including document lookup and warmed candidates. Existing owned-descriptor tests continue to cover API compatibility.

Whole-workspace Clippy with warnings denied is checked in the existing CI configuration; the new test/example are also checked with allocation counting enabled. An exploratory allocation-enabled **all-targets** run reports the pre-existing `needless_question_mark` in unchanged `dev/bench/src/bin/font_match_probe.rs:224`. That unrelated optional probe's lint is not changed here; it is not a runtime failure. This scope choice can leave that lint failing under Clippy 1.97. The workspace, allocation, feature and CI results are recorded in the final JSON/Plane outcome.

## Reproduce

Use two isolated worktrees at the baseline and measured fix commits. Copy the fix's probe source to the baseline worktree; it uses only public APIs available at both revisions. Keep build directories separate, and preserve the timing executable before rebuilding it with allocation counting.

```sh
PATH="$HOME/.cargo/bin:$PATH" TMPDIR="$HOME/tmp" RUSTC_WRAPPER="" \
CARGO_TARGET_DIR="$HOME/tmp/shodo-2-baseline-build" \
CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_INCREMENTAL=false \
cargo +stable build -p shodo-bench --release --example font_descriptor_lookup

taskset -c 0 "$HOME/tmp/shodo-2-baseline-build/release/examples/font_descriptor_lookup" \
  prepared-heavy 20000 2 5

PATH="$HOME/.cargo/bin:$PATH" TMPDIR="$HOME/tmp" RUSTC_WRAPPER="" \
CARGO_TARGET_DIR="$HOME/tmp/shodo-2-baseline-build" \
CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_INCREMENTAL=false \
cargo +stable build -p shodo-bench --release --features allocation-counting \
  --example font_descriptor_lookup

taskset -c 0 "$HOME/tmp/shodo-2-baseline-build/release/examples/font_descriptor_lookup" \
  prepared-heavy 1000 2 3
```

Repeat for the fix and `prepared-small`, `raw-static`, `cache-hit`; alternate revision order by pair. Select an available CPU on other machines. Validation uses `cargo +stable test --workspace`, the new allocation test in debug/release, `cargo +stable fmt --all --check`, `git diff --check`, and CI's feature/snapshot/MSRV/Wasm checks. Temporary measurement data and both task worktrees are removed only after this record and the Plane outcome are saved.
