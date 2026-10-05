# shodo-mc0 Fail-Closed Ruby Line Measurement Work Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bound the ruby line-measurement work of every `next_line` and `intrinsic_sizes` call by a work allowance linear in what the call scans, degrading deterministically with a warning (never an error) beyond it, and bound the memory of the `blocks` / `block_effects` range caches.

**Architecture:** Two independent pieces. (A) `RangeCache::blocks` and `block_effects` get an entry cap; reaching it clears both maps before the next insert and releases large capacity. Since shodo-tj5 a hit has exactly the side effects of a miss, the cap changes no output or warning. (C) A per-operation `LineWork` on the `LayoutContext` charges one unit per live ruby container measurement (`measure::measure_one`) and per position added one at a time by an accumulator's saturating replay. Its allowance is `Limits::max_ruby_line_work` (a factor) times the longest probed range (`end - start` units) plus the widest container walk seen in the operation. `candidate_adjustment` checks it on entry to the reuse path: once spent reaches the allowance, the rest of the operation answers every adjustment-only probe with zero and pushes one `WarningKind::Unsupported`. Accepted lines still measure their ruby in full (`candidate` via `apply`), so placed ruby geometry stays exact; only fit decisions and intrinsic sizes ignore annotation overflow.

**Tech Stack:** Rust (crate `shodo`), cargo unit tests with test-only counters, release probes under `dev/bench` (`mc0_scale`).

**Spec:** the `design` field of beads issue `shodo-mc0` (`bd show shodo-mc0`). Records in Japanese, source comments in English.

## Global Constraints

- The degradation is a function of (paragraph, atomic revision, operation inputs, warning sink suppression) only: never of caches left by earlier operations (`RangeCache`, edge windows, a retained `PartialLine`).
- No new error: `break_all` is infallible. Exceeding warns once per operation and degrades.
- With default limits, every existing fixture and probe keeps byte-identical output and warnings.
- The reference path (`cx.ruby_reference`) never degrades; it is the exact oracle.
- The allowance does not depend on how often a probe repeats (it uses maxima), so the probes of `PartialLine::index` (a superset of a cold narrower scan's) never trip later than the cold scan.
- No attribution lines in commits. Never touch untracked files in the main checkout or `.worktrees/shodo-9an-2`.
- Gates before each commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p shodo --lib` (the full workspace suite before the PR).

## Review Focus

1. Determinism: a degraded operation run cold, warm (caches filled by earlier calls) and after a retained wider `PartialLine` gives identical lines and warnings.
2. A failed speculative `PartialLine::index` restores the work counter and clears the per-operation ruby memo, so its fresh rescan starts like a cold call.
3. Overshoot: one admitted probe can exceed the allowance by at most the containers it walks.
4. The mid-operation reset (`ruby_line` → `begin_reshape_operation`) happens after every fit probe and resets the work state with the reshape budget.
5. `blocks` cap: crossing it mid-operation keeps `(height, Saturation, spent, warnings)` equal to a fresh context.

---

## File Structure

- Modify `crates/shodo/src/limits.rs`: `max_ruby_line_work` field, default, `unlimited`.
- Create `crates/shodo/src/ruby/line_work.rs`: `LineWork` state, `admit`, `charge`.
- Modify `crates/shodo/src/context.rs`: field, reset in `begin_reshape_operation`.
- Modify `crates/shodo/src/ruby/measure.rs`: admit in `candidate_adjustment`, charge in `measure_one`.
- Modify `crates/shodo/src/ruby/accumulate.rs`: charge the sequential replay.
- Modify `crates/shodo/src/line/cache.rs`: restore on a failed `index`.
- Modify `crates/shodo/src/line/range.rs`: `MAX_BLOCKS` cap and release.
- Tests in `crates/shodo/src/ruby/tests/` (new `line_work.rs`) and `line/range.rs`.
- Modify `docs/guides/integration.md`; create `docs/records/shodo-mc0-ruby-line-work.md`; add `dev/bench/examples/mc0_scale.rs`.

---

### Task 1: `blocks` cap (A)

- [ ] Test `block_cap_crossing_matches_a_fresh_context`: with a test override of the cap (2), query every range of a charging fixture twice; per query `(height, sat, spent, warnings)` equals a fresh context's, and the maps never exceed the cap.
- [ ] Implement `MAX_BLOCKS` (16,384 entries over both maps), clear both before an insert that would exceed it, `shrink_to` when the capacity exceeds `RETAINED_BLOCKS` (256).

### Task 2: Work counter and limit (C)

- [ ] Failing tests: `adversarial_churn_is_bounded` (container measures of `profile_churn` grow linearly past the trip point, one warning, `max_ruby_line_work: None` keeps the old growth), `degraded_lines_match_cold_warm_and_retained` (cold, warm, after a wider retained `PartialLine`), `zero_factor_fits_without_ruby` (equals the reference with zero adjustments), `default_limit_keeps_fixtures_exact` (d77/2j6/b7d fixtures: no warning, same lines as `None`).
- [ ] `Limits::max_ruby_line_work: Option<u64>` (default chosen from Task 3's measurements), `LineWork`, admission in `candidate_adjustment`, charge in `measure_one` and the sequential replay, restore in `index`.

### Task 3: Calibrate the default

- [ ] Test-only `max_ruby_work_ratio` tracker: over the lib test suite and the probes, the highest `spent / (span + walk)` an operation reaches. The default is a power of two well above it.
- [ ] Release probe `mc0_scale`: siblings 16000/17000/32000, churn 500–8000, ordinary/plain controls, intrinsic; with and without the limit.

### Task 4: Docs and record

- [ ] `docs/guides/integration.md`: the allowance, the degradation and the warning.
- [ ] `docs/records/shodo-mc0-ruby-line-work.md`: remaining shapes and bounds, decision, compat, A/B results.
