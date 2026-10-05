# shodo-tj5 Cache-Free Ruby Measurement Charges Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every `RangeCache` query in ruby line measurement charges the reshape budget and `Saturation` exactly as a cold query of the same paragraph and range would, so accounting no longer depends on what an earlier query left in the cache.

**Architecture:** Three cached sites, three mechanisms. `blocks` (`block_size`) records the `line::replay::Effects` of its cold measurement and replays them on a hit through the exact replay gate, re-measuring when the gate refuses. `sets` (`build`) stops charging the caller: per-unit saturation is recorded sparsely and each `width` query charges the sum over its own units. The tab prefix stores cumulative per-step saturation and every query charges the steps of the tabs before its end, whether they were computed now or earlier. With effects independent of cache state, `RangeCache::generation` and `fills` lose their meaning and are removed together with the memo and accumulator gates built on them; `epoch` (root change, `vacate_slots`) stays as a conservative guard.

**Tech Stack:** Rust (crate `shodo`), cargo unit tests with test-only counters, release A/B probes under `dev/bench`.

**Spec:** the `design` and `acceptance` fields of beads issue `shodo-tj5` (`bd show shodo-tj5`). Records in Japanese, source comments in English.

## Global Constraints

- Charges are a function of (paragraph, atomic revision, range, `edge_reshape_spent`, sink suppression) only: never of cache contents.
- Reference (`cx.ruby_reference`), Memo (`cx.ruby_accumulate_disabled`), Accumulate (default) and Verify (`cx.ruby_accumulate_verify`) must still agree on values, warnings and saturation.
- Saturation counters are added and subtracted with wrapping arithmetic (`line/replay.rs`), and effect flags are never derived from wrapped totals.
- Memory: a clean paragraph pays nothing new. A `blocks` entry without effects keeps today's size; only entries with effects carry an `Effects`. Saturation records are sparse (saturating units, saturating tab steps).
- Linear guards of shodo-d77, shodo-2j6 and shodo-b7d keep passing unchanged.
- No attribution lines in commits. Never touch untracked files in the main checkout or `.worktrees/shodo-9an-2`.
- Gates before each commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p shodo --lib` (the full workspace suite before the PR).

## Review Focus

1. A `blocks` hit whose recorded charges no longer replay (budget near its limit, suppression changed): must re-measure, and the result (value, warnings, spent) must equal a cold measurement.
2. A `blocks` measurement that pushed a warning: never stored, so repeated queries warn as cold queries do.
3. Wrapping saturation prefixes (sets and tab steps): differences use `wrapping_sub`.
4. Tab prefix replacement under a saturating `tab-size` (`1e12px`): epoch must not move any more, and the memo/accumulator must keep replaying (shodo-b7d's remaining quadratic shape).
5. Speculative `PartialLine::index` rollback: unchanged reasoning; the cache-state clause in the memo/accumulator docs is replaced, not just deleted.

---

## File Structure

- Modify `crates/shodo/src/line/range.rs`: `RangeCache` (remove `generation`/`fills`, add `block_effects`), `block_size`, `Costs`/`build` (sparse saturation), `TabPrefix`/`width` (cumulative step saturation), tests.
- Modify `crates/shodo/src/line/testdata/b7d_tab_golden.txt`: regenerated on purpose; the diff is recorded.
- Modify `crates/shodo/src/ruby/measure.rs`, `crates/shodo/src/ruby/memo.rs`: drop the generation gate and its docs.
- Modify `crates/shodo/src/ruby/accumulate.rs`: drop the `fills` storable gate; rewrite the "Cache state" docs.
- Modify `crates/shodo/src/ruby/tests/memo.rs`, `crates/shodo/src/ruby/tests/accumulate.rs`: rewrite tests that encode the old rule; counts report drops `fills`.
- Create `docs/records/shodo-tj5-cache-free-charges.md`.

---

### Task 1: Failing tests for cache-state independence

**Files:** `crates/shodo/src/line/range.rs` (tests)

- [ ] Add `block_size_charges_the_same_cold_and_warm`: a paragraph whose block measurement charges the reshape budget (`Limits::default()`, a source-clipped edge window). For each of cold, warm hit, and cold again after `vacate_slots`, reset `edge_reshape_spent` to the same value, use a fresh `Saturation`, and assert equal `(height, sat, spent delta)`, with a non-zero spent delta.
- [ ] Add `width_charges_do_not_depend_on_query_order`: for a paragraph with a saturating unit and saturating tabs (`TabSize::Px(1e12)`), run a range history forwards in one context and backwards in another (and each query alone in a fresh context); assert per-range `(value, sat)` identical across the three.
- [ ] Run; both fail on main (warm charges less).

### Task 2: `blocks` replay

- [ ] Add `block_effects: FastMap<BlockKey, (LayoutUnit, Effects)>` beside `blocks`. Miss path: `replay::begin`, measure, `replay::finish`. `Some(e)` with no charges, clean sat and unsuppressed: insert into `blocks`; `Some(e)` otherwise: insert into `block_effects`; `None`: remove any entry.
- [ ] Hit path: `blocks` returns directly; `block_effects` replays with `replay::replay`, falling through to the miss path when refused.
- [ ] `Effects::is_empty` helper in `line/replay.rs` (no charges, clean sat, unsuppressed). Clear `block_effects` with `blocks`.

### Task 3: `sets` sparse saturation and tab step saturation

- [ ] `build` measures each non-atomic, non-tab unit's width and word spacing with a local `Saturation`; units with a non-clean local sat are pushed to `Costs::saturated: Vec<(usize, Saturation)>` with cumulative wrapping counts. `build` no longer takes `sat`.
- [ ] `width` charges the cumulative difference of `saturated` over `range.start..range.end`.
- [ ] `TabPrefix`: replace `effects` with `sat: Vec<Saturation>` (cumulative per covered tab, empty while every step is clean, backfilled on the first saturating step). Step computation charges nothing; after the prefix covers `range.end`, charge the cumulative saturation of covered tabs before `range.end`.
- [ ] Remove `discards_effects`/`computed_effects`, `fill()`, `fills`, `generation`.
- [ ] Task 1 tests pass.

### Task 4: Memo and accumulator gates

- [ ] `ruby/measure.rs`: memo replays without a generation check; a recording is stored whenever it pushed no warning. `MemoEntry::generation` removed. Memo docs: cache-state clause rewritten (charges are cache-independent; the window cache already charged on hits).
- [ ] `ruby/accumulate.rs`: `storable = self.replays && !note.mixed`; "Cache state" docs rewritten (epoch only).
- [ ] Rewrite old-rule tests: `generation_splits_into_monotone_fills_and_invalidating_epochs` → epoch only; `effectful_tab_steps_fill_and_their_discard_invalidates` and `mixed_tab_prefix_latches_effects_when_a_later_step_saturates` → covered queries charge the same as computed ones and replacement keeps the epoch; `effectful_tab_prefix_replacement_stops_replay` → keeps replay (no resets), matches reference; `alternating_tab_starts_keep_entries_of_the_current_epoch` → epoch unchanged; `entries_recorded_while_caches_fill_are_unstored` → stored on the first pass; memo test "a cold recording is measured again" → replayed on the second probe.
- [ ] Add a linear guard for a saturating tab (`tab-size: 1e12px`) in nested and sibling shapes.
- [ ] Regenerate `b7d_tab_golden.txt` with `SHODO_UPDATE_GOLDEN=1`; check that only saturation columns change and only on `huge` (and record the diff).

### Task 5: Behavior and performance check

- [ ] Build `tab_scale`, `sibling_scale`, `d77_scale` release binaries for main and candidate in separate target directories; compare output and warning digests per case and size.
- [ ] Interleaved A/B (`perf stat -r 1 -e task-clock`, ABBA) on the ruby workloads; raw data under the session scratchpad, not committed.
- [ ] Operation counts via `b7d_operation_counts_report` (now without `fills`).

### Task 6: Record, gates, review, PR

- [ ] `docs/records/shodo-tj5-cache-free-charges.md` (Japanese): cause, change, behavior diffs (warnings/numbers), perf, remaining shapes.
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- [ ] Code review, fixes, PR, CI, merge, cleanup, close; mc0 comment.
