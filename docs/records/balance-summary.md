# Balance summary retention

Issue: shodo-c91.5. Decision: adopt streamed endpoint retention; preserve complete post-scan.

Balance needs line counts and mapped ends. Its private fixed-width iterator now releases each completed Line immediately and retains u32 endpoints. The initial greedy count, bounded Q26.6 width sequence, full scanning and warning effects stay unchanged. Start/Pretty retain their existing full-Line path. Constructors, metrics, Ruby placement and shapers still run; this change removes simultaneous trial geometry retention, not their work.

The forced-atomic regression observed root owner peaks65→2 with64 constructors, zero clones and32 ordered16x10 atomic outputs. The infeasible-trial test scans all2+4 Lines and preserves the tail missing-atomic warning. Owner peaks are Arc strong counts observed at Line constructors, not total live-Line counts or geometry bytes.

## Comparison

Before public source: 9b59c13407999d953ea13bd3fed2e3618062a18c. Candidate runtime source fingerprint: 2212097e952ba33ebb3a58b31b908d97d024c6ff634bc8ee051029d4cf53731a. Published runtime and benchmark files are byte-identical to the measured candidate.

All117 fixed-font Start/Balance/Pretty conditions retain complete recursive source/glyph/font/geometry, public events, plans, ordered warnings and needed height. Controls cover first-line/Ruby/forced/block/float, zero/one/default/None iteration limits, actual font-layer cache disablement, short shaping runs, edge fallback, warning suppression and saturated width. Atomic/generation contracts are also covered by core regressions.

Requested allocation scopes are build, plan, accepted and four separate release windows for output, plan, paragraph and context. Builder preparation, font registration, snapshots and warning draining occur outside scopes. All seven scope counters match memory/observer builds. Requested calls/gross/freed/net/peak are separate from RSS. Actual work and complete trial widths/counts are unchanged.

Four whole-matrix and8 selected cases×4 fresh processes use alternating before/after/after/before order, two warmups and seven samples. The table uses Plan medians of per-process medians. Ratios below1 are faster; no universal CPU or process-memory guarantee is made.

| Condition | Fresh Plan after/before | Whole-matrix ratio | Gross bytes before → after | Plan peak before → after | Plan net before → after |
|---|---:|---:|---:|---:|---:|
| plain/32/80/default/Balance | 0.9639 | 0.9322 | 6690286 → 5249678 | 374751 → 122485 | 119402 → 119530 |
| ruby/32/80/default/Balance | 1.0028 | 1.1486 | 30064103 → 29401943 | 1523673 → 1108669 | 1100397 → 1100493 |
| first-line-ruby/32/80/default/Balance | 0.9922 | 1.1187 | 62538958 → 61876774 | 1523909 → 1106737 | 1100353 → 1100445 |
| float/32/80/default/Balance | 0.9613 | 0.9499 | 7106222 → 5665614 | 375007 → 122517 | 119402 → 119530 |
| plain/1/80/default/Balance | 1.0034 | 1.0557 | 158847 → 134443 | 12826 → 5151 | 2656 → 2660 |
| plain/1/80/disabled/Balance | 1.0178 | 1.2540 | 14947 → 13415 | 5666 → 3854 | 1563 → 1567 |
| plain/32/80/default/Start | 1.0245 | 0.9986 | 527068 → 527068 | 134719 → 134719 | 43024 → 43024 |
| ruby/32/80/default/Pretty | 0.9877 | 0.9287 | 4225606 → 4225606 | 1273849 → 1273849 | 1100397 → 1100397 |

Across39 Balance conditions, calls/gross/peak/net increase in 0/0/0/39 conditions. Endpoint Vec capacity grows instead of retaining an exact-length allocation, so final endpoint storage rises slightly. Independent probes verify release frees capacity×4 bytes without allocating, and root lifetime remains unchanged while BreakPlan holds only an identity. Adoption rests on reduced temporary geometry retention.

Largest whole-matrix ratios, including unaffected controls: first-line-ruby/1/80/default/Balance=1.8560, ruby/1/240/default/Pretty=1.6231, first-line/32/240/default/Balance=1.5857, plain/1/80/run-one/Balance=1.5633, ruby/32/240/default/Pretty=1.5402.

## Validation and reproduction

Core430 passed/2 ignored; fmt, workspace all-target Clippy, docs, allocator/probe and dedicated harness checks passed. All54 default conditions retain seven operation digests. Six pinned producers distinguish counter-free timing, requested allocation and allocation-neutral work observations. A missing Start/Pretty greedy-count observation in the initial observer was corrected without changing production/time/memory producers; its original trace and failing replay remain in the local evidence.

The public manifest data/balance-summary.json contains aggregate results only. Raw streams, source/producer logs and machine metadata remain in local evidence as requested and are outside this PR. Complete raw byte restoration and independently recomputed medians passed before this publication-only reduction; the public summary is checked against those unchanged private measurements.

To reproduce, use the published balance_summary example and existing completed_height_snapshot helper with the repository fixed fonts and licenses. Run the same example on the before source and this runtime, using the common harness, identical release settings and font limits. Separate allocation-counting from timing; complete checks and output comparison before balanced timing. SHODO_BALANCE_SAMPLES sets sample count, SHODO_BALANCE_CASE selects a case and SHODO_BALANCE_REVERSE changes condition order.

No original diagnostic or saved spike is modified, and this issue does not block a production switch.
