# SHODO-6: guarded Balance trial summaries

**Decision: adopt a bounded summary path for eligible Balance trials.**
Root-only horizontal text without prepared geometry can return its mapped
break token and exact public height before constructing a completed `Line`.
All shared scanning, shaping, spacing and alignment effects still run.
Unsupported lines finalize completely and then reduce to the same two values.
Public Full formatting and Pretty retain completed geometry.

This is a partial implementation of the broader trial-geometry proposal.
First-line paragraphs, ruby, floats, inline boxes, combined/vertical text,
emphasis, controls, hidden soft hyphens, sliced/empty glyphs, prepared overlays,
spacing positions and numeric outliers keep the existing full path. CJK needs
spacing positions even when individual gaps are zero, so it is currently a
full fallback. No general ruby or Japanese speedup is claimed.

The [JSON record](shodo-6-balance-trial-summary-2026-10-12.json) preserves every
timing sample, constructor/allocation diagnostic, output checksum, fixture hash,
measurement source and ruling. Large repeated output snapshots are represented
by verified canonical SHA-256 values; the fixed benchmark reproduces their full
contents and asserts public geometry equality. Raw timing series are retained
without duplicate snapshots.

## Effects and numeric proof

Private Full/Trial finalizers share the existing formatter state machine.
Trial fallback preserves all non-line result fields. First-line results are
reduced only after the existing alternate-to-normal cursor mapping. The trial
driver preserves float cursor progress and block boundaries, scans every
infeasible trial to its end, and uses the same public-height round trip as the
public iterator when advancing block offsets. Balance widths/iteration limits
are unchanged; Pretty still uses its completed selected-line machinery.

Summary success requires a clean saturation state and an available, unchanged
warning checkpoint after all prefix effects. Selected units must be ordinary,
nonempty glyph clusters in the root style, with no parent, shared cluster or
combination, baseline alignment and the paragraph's base bidi level. Relevant
font sizes/metrics, root extents, record profiles and annotation content extents
must be finite and bounded by 4096 px. Advances are nonnegative and total at
most 2^20 px; origin/content/hanging coordinates are also bounded. There are no
profile groups or nonzero baseline shifts.

Within that class, existing `ProfileResolver::record` gives exactly the full
glyph extents. A default resolver has the same parent lookup as `new` because
every parent is None, and allocates no map. Adjacent equal run/item profiles are
independent of text/glyph ranges in this class. Extent union, nonnegative height,
ceil height and rounded baseline follow the full solver; retained conversions
are checked in local Saturation. Failed probes never reset budgets or write
context warnings.

Discarded fragment coordinates stay below 2^22 px, including f32 conversion
error in the eligibility bounds. Annotation/metric arithmetic
uses a fixed number of bounded terms below 2^16 px. Both are well below Q26.6's
approximately 2^25 px saturation boundary. The complete path runs for anything
outside the proof boundary, preserving its numerical warnings. No geometry is
cached in a successful summary.

## Measurements

Baseline: `cc3aa5b6b3844c995c92831fa4c114ca03873d61`.
Runtime candidate: `65ee74411109be5fcce320cb9dd3772f1f00d5a7`.
Both revisions use the same expanded `balance_summary` benchmark from this PR.
Linux x86_64, AMD Ryzen 5 5600G, performance governor, CPU 0, Rust 1.97.1;
release debug=0/incremental=false, other Cargo defaults, no rustc wrapper.
Fixed Latin/CJK/Arabic font registration and exact font bytes come from
`dev/fixtures`; identities/hashes and executable hashes are in JSON.

The 165-case matrix covers narrow/wide, one/32 repetitions, plain, first-line,
ruby, first-line ruby, forced/block/float boundaries, CJK, combined, vertical
and styled paragraphs; it includes zero/one/default/unlimited search and tiny
reshape/font/run/warning limits. Start and Pretty serve as public controls.
Ten timing processes use five alternating baseline/candidate pairs, reversing
case/mode order in alternating pairs. Each case has two warmup runs and three
samples per process, giving 15 samples per revision/case. Paragraph build,
planning, accepted geometry and release are separate windows. No counting
allocator or observer appears in timing executables. Three-sample allocation
diagnostics use separate builds; their times are not speed evidence.

Every matrix output, plan end, height-rejection result and ordered warning
matches across all timing and diagnostic processes. Complete snapshots include
glyph/cluster/font/source mapping, recursive ruby, combination and box geometry,
annotations, offsets and float/block events. Owner IDs are normalized by the
existing snapshot helper; numeric/output fields are compared without rounding.

| Case, repetitions / width | Median plan ns, baseline → candidate | Ratio | Completed Lines, baseline → candidate; summaries |
| --- | --- | --- | --- |
| plain, 1 / 80 | 143,391 → 133,193 | 0.929 | 51 → 15; 36 summaries |
| plain, 1 / 240 | 87,306 → 84,023 | 0.962 | 21 → 4; 17 summaries |
| plain, 32 / 80 | 4,644,522 → 4,333,993 | 0.933 | 1,632 → 480; 1,152 summaries |
| plain, 32 / 240 | 2,135,567 → 1,921,563 | 0.900 | 470 → 6; 464 summaries |
| ruby, 32 / 80 | 20,552,733 → 20,858,654 | 1.015 | 1,204 → 1,204; no summaries |
| first-line, 32 / 80 | 6,022,415 → 6,007,608 | 0.998 | full fallback |
| float, 32 / 80 | 5,759,730 → 5,763,082 | 1.001 | 1,632 → 1,632; no summaries |
| CJK, 32 / 80 | 1,794,166 → 1,782,433 | 0.993 | 752 → 752; no summaries |
| combined, 32 / 80 | 2,812,290 → 2,773,666 | 0.986 | 843 → 843; no summaries |
| vertical, 32 / 80 | 1,861,357 → 1,848,436 | 0.993 | full fallback |
| styled, 32 / 80 | 5,741,291 → 5,768,111 | 1.005 | full fallback |

Plain Balance has 14 fixed-width passes at width 80 and 15 at width 240,
including initial greedy; baseline/candidate pass counts agree. Constructor
counts include recursively formatted children. Fallback timing changes are
small in these fixtures and do not establish an improvement.

| Plain Balance case | Allocation calls | Requested traffic, bytes | Retained net / extra peak, bytes |
| --- | --- | --- | --- |
| 1 / 80 | 1,401 → 1,257 | 157,973 → 144,485 | 2,460 / 4,919 unchanged |
| 1 / 240 | 559 → 491 | 58,417 → 51,869 | 3,064 / 4,294 unchanged |
| 32 / 80 | 49,619 → 45,011 | 5,776,276 → 5,344,660 | 83,618 / 86,548 unchanged |
| 32 / 240 | 6,752 → 4,890 | 1,267,614 → 1,084,574 | 5,894 / 8,776 unchanged |

Summary saves allocation traffic, rather than measured retained bytes or peak
memory. Ruby, CJK, combined and float diagnostics have identical allocation
counts/traffic/net/peaks. These are allocator-requested bytes, not RSS or usable
allocation size. Shaping/scanning dominate much of the remaining plan time;
98.7% fewer wide plain constructors does not mean a comparable time reduction.

## Accepted cost and controls

The initial matrix showed slower accepted geometry for small plain Balance,
so six cases were rerun with five alternating pairs and 50 samples per process
(250 samples per revision/case), using the saved counter-free binaries. These
are separate confirmations, not pooled with the matrix.

| Targeted case | Median plan ns, baseline → candidate | Accepted ns, baseline → candidate |
| --- | --- | --- |
| plain 1 / 80 Balance | 145,591 → 131,203 | 8,591 → 9,010 |
| plain 1 / 240 Balance | 88,877 → 81,579 | 5,238 → 5,657 |
| plain 1 / 240 Start | 8,521 → 8,590 | 4,819 → 4,819 |
| plain 1 / 240 Pretty | 10,546 → 10,581.5 | 4,819 → 4,819 |
| plain 32 / 80 Start | 440,999 → 441,977 | 191,513.5 → 194,657 |
| plain 32 / 80 Pretty | 812,082.5 → 807,752.5 | 192,421.5 → 195,600 |

Small Balance accepted time rises 4.9–8.0%, repeatedly across the five pairs;
it is not dismissed as noise. Planning saves more than that phase loses:
the sums of stage medians fall approximately 9.1% and 7.3%, respectively.
Normal Start/Pretty plan changes are -0.5% to +0.8%; small accepted medians
match, large accepted medians rise 1.6%. Combined normal control stage medians
change by less than 1%. These fixtures support the bounded optimization's
adoption, not an exact zero-regression guarantee or cheaper accepted output.
The exact cause of the accepted-phase increase is not established; code
generation, cache/allocator state and stage interaction remain possible.

## Verification and reproduction

The real synthetic-TTF root-text regression observed 48 completed constructors
with summaries disabled, then zero with the summary finalizer enabled. Separate
tests compare public completed heights/tokens, warm contexts, zero/fractional
line heights, tiny reshape/warning budgets, numeric root/run font outliers and
warning suppression. They prove CJK fallback and continued Full/Pretty
materialization. First-line transform/remap, float/block/forced progress,
infeasible tail warnings and existing geometry/budget suites remain covered.
Task 2 full crate verification: 1,264 passed, zero failed, nine ignored;
the focused line suite passed 200 tests. Final workspace verification passed
1,796 tests with zero failures and nine ignored. Formatting and workspace
Clippy with and without accesskit passed with warnings denied. The
allocation-counting example configuration is checked separately; final
CI/review results are recorded after their completion.

For timing, check out the baseline and candidate into separate task worktrees
and copy this PR's `dev/bench/examples/balance_summary.rs` into the baseline.
Use distinct target directories under `~/tmp`:

```sh
TMPDIR="$HOME/tmp" RUSTC_WRAPPER="" CARGO_TARGET_DIR="$HOME/tmp/shodo6-target" \
  CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_INCREMENTAL=false \
  cargo +stable build --release -p shodo-bench --example balance_summary
SHODO_BALANCE_SAMPLES=3 taskset -c 0 \
  "$HOME/tmp/shodo6-target/release/examples/balance_summary" > results.json
```

Alternate revision order five times; set `SHODO_BALANCE_REVERSE=1` in odd pairs.
The embedded measurement drivers reproduce the matrix and targeted protocol.
For allocation/construction diagnostics, apply the embedded disposable observer
to both worktrees and add `--features allocation-counting`. Observer counters
record actual constructors, summary successes and completed fixed-width passes;
remove them before shipping. Keep timing binaries separate from diagnostics.

The Start oracle originally compared collection-level `break_all` geometry with
iterator-level planned geometry; only `break_all` assigns box slice offsets.
It now compares the same public iterator with and without a plan, preserving
complete field comparisons. Existing collection slice-offset tests are
unchanged. The plan's nonexistent `docs/records/README.md` index was corrected
to the repository's `docs/README.md`. These rulings and the CJK eligibility
correction are retained in JSON for review and handoff.
