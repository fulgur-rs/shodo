# Variable font warm-match diagnosis — shodo-sbp.10

The existing hash-indexed LRU already avoids a linear cache lookup. This work measures the remaining nonempty `FontMatch.variations` Vec clone on normalized/scripted internal hits and evaluates sharing the immutable result. Public `FontMatch` still owns `Vec<FontVariation>`; public caller mutation stays independent. Primary/public matching uses `Arc::unwrap_or_clone`, while itemize and retained shape items use a private Arc. Hyphen substitution normalizes its local query once and uses the same private result.

## Fixed inputs and scopes

Roboto Flex bytes are unmodified, SHA256 `9b523f7d82593df0107173849ebb8c817471a1df4b4fb2c3cbf40cfd810c8281`, 1,787,292 bytes, Google Fonts commit `23e54b51ddffbc7713c583748e3bd86f62b1fa4a`, Git blob `2a11e4cd5588a89e0047140b09c912059d1a150f`. The original OFL accompanies the archived input. Real axes include wght100–1000, wdth25–151, slnt−10–0 and opsz8–144. There is no ital axis: real italic style maps to slnt; the synthetic five-axis CI fixture tests ital conversion independently. Existing fixture Latin is the static control, byte-identical to the checked-in fixture. System discovery is disabled and default limits remain unchanged.

Seventy-two dedicated conditions comprise54 match conditions (variable/static × normal/oblique/italic ×9 operations) and18 public paragraph builds. Operations distinguish internal warm, public warm, negative cached result, retained results,64 styles with cap1024/8, disabled cap0, cold miss, and generation updates. Paragraphs use1024/4096/16384 characters,8/64 alternating styles, explicit wght/wdth/slnt/opsz, automatic optical sizing and size-adjust. Each build verifies warnings, source mapping, glyphs/geometry, face identity, instance size, normalized coordinates and variations through an exact payload SHA256 plus representative instance data. All expected glyph counts are literal input lengths and every glyph ID is nonzero.

Matching scopes exclude registration, query preparation, prewarm, reference validation and retained-result Vec capacity. Transient internal results are dropped in scope; the public final result or all retained results stay live until scope end. Generation scopes also include fallback updates. Paragraph scopes include builder/text/style preparation and warm build, with paragraph retained; registered font, warmed LayoutContext, later line layout and validation are excluded. Absolute live bytes contain outside-scope retained data; compare calls/gross/net/extra peak rather than those absolute values. RSS is unmeasured.

A documented development-only source overlay exposes a forwarding boundary for private normalized matching. Optional counters record actual hits/misses. Counter-free timing, allocation counting and hit/miss tracing are three separate immutable executables. The allocator is a byte-identical copy of the existing shodo-bench allocator; registry versions match the workspace lock. No diagnostic entry point ships in the product API. Initial pilot data remains archived: explicit post-scope private/public output verification was added before canonical baseline capture without changing measured operation bodies.

Baseline production is main `2dcc9f57a5211566d3505f74f867af96d1d94b58`, source fingerprint `ed629685f7919a4633725d326859f3492b2614141fd5cbf40d6daf69228ca1fd`. Candidate implementation is `d272c271e3ca97a236c309f327bc1b860765b6b9`, source fingerprint `5fb9d61191ba012385766c0a6605c43f5bd9a606ecacca576141875972c16a2f`; the immutable capture records its equivalent preceding HEAD plus full tracked diff. Standard harness fingerprint remains `7c139740f9da6bc5ab72420426c81ad5456569e36a99b18b2266fe392b18f80b`.

## Allocation and retention results

All72 dedicated output signatures and actual hit/miss counts match; three allocation repeats are deterministic. Normal internal warm10000 has10000 hits/0 misses; the negative control also has10000 hits/0 misses. No timing value from the trace binary supports a speed claim.

| Scope, variable normal | Calls before → after | Gross bytes before → after | Net bytes before → after | Extra peak before → after |
|---|---:|---:|---:|---:|
| Internal warm10000 | 10000 → 0 | 240000 → 0 | 0 → 0 | 24 → 0 |
| Public warm10000 | 30000 → 30000 | 530000 → 530000 | 24 → 24 | 77 → 77 |
| Retained1000, preallocated caller Vec | 1000 → 0 | 24000 → 0 | 24000 → 0 | 24000 → 0 |
| Cold64 distinct styles, cap1024 | 848 → 848 | 102612 → 100212 | 17424 → 19472 | 18465 → 20441 |
|64-style thrash, cap8,1024 operations | 13312 → 13312 | 1381248 → 1422208 | 0 → 0 | 1183 → 1183 |
| Disabled cache,256 operations | 2560 → 2816 | 314368 → 330752 | 0 → 0 | 1183 → 1183 |
| Warm plain16384 build | 33103 → 16719 | 11874316 → 11480940 | 4429341 → 4429157 | 4576726 → 4576542 |
| Warm1024 characters/64 styles build | 28193 → 27169 | 7945529 → 7839193 | 1558270 → 1492734 | 2272615 → 2207079 |

For plain16384, eliminating one short-lived variation clone per grapheme reduces call count about49.5%, gross about3.3%; this does not establish dominance of total CPU or net retention. Miss/cap0 costs include allocating the shared match, and each cached match has additional heap ownership. Cache entry storage shrinks but cold64-style variable net retention rises2048 bytes. The caller's preallocated retained-result Vec lies outside the dedicated retained scope; its type also changes privately and no whole-caller retained footprint is inferred from net0.

Static internal warm remains0 allocations; public warm stays20000 calls/290000 gross bytes. Static cold64-style matching rises592→656 calls and net15888→17424 bytes; cap0 rises1792→2048 calls and gross238592→254976 bytes. Static plain16384 build retains16696 calls, gross11480380→11480220, net4429243→4429083. Sharing is not uniformly free across fonts or workloads.

## Correctness verification

The actual Clone counter first failed with1 nonempty variation clone on a warm internal hit, after literal four-axis values were verified; it then passed with0. Matching12, itemize19, shape21 and actual hyphen7 tests pass. Workspace1082, no-default629, complex-only632, fmt, all-target Clippy with denied warnings, docs with denied warnings and fixed glyph snapshots pass on the frozen candidate. Public owned mutation, descriptor/axis clamps, italic/slant conversion, bounded/disabled/negative caches, shared/document generation invalidation and retained old values are covered. The initial `line::hyphen` selector ran0 tests; that log is preserved and the corrected global hyphen selector ran7 actual tests.

## Counter-free time and adoption

Rust1.97.1 release opt3/debug0/incremental false, CPU10, same fixed font/config/harness. After local builds, each immutable binary gets one warmup run, then before/after/after/before fresh processes. Each process has10 samples per condition. The values below are each process's sample median, not a pooled distribution or statistical confidence interval. Full72-condition medians and all samples remain archived.

| Scope | Before run medians, ms | After run medians, ms |
|---|---:|---:|
| Variable internal warm10000 | 0.5076 / 0.4943 | 0.3094 / 0.3061 |
| Variable public warm10000 | 0.7848 / 0.7801 | 0.7541 / 0.7481 |
| Variable cold64 styles | 0.04746 / 0.04715 | 0.04641 / 0.04666 |
| Variable disabled256 | 0.1164 / 0.1178 | 0.1153 / 0.1187 |
| Variable64-style cap8 thrash1024 | 0.5826 / 0.5905 | 0.5762 / 0.5852 |
| Variable plain16384 build | 12.508 / 12.541 | 12.103 / 11.962 |
| Variable1024 characters/64 styles build | 7.052 / 7.024 | 6.930 / 6.914 |
| Static internal warm10000 | 0.4040 / 0.4038 | 0.3192 / 0.3133 |
| Static cold64 styles | 0.03936 / 0.03960 | 0.04055 / 0.04048 |
| Static plain16384 build | 8.392 / 8.423 | 8.498 / 8.469 |

Adopt the private shared result for the existing cached shaping path. The chosen variable warm path removes transient allocation, and repeated variable plain16384 build is about3.9% faster in this host experiment. Cold/static/disabled costs and retained data remain visible: this is not a uniform speed or memory reduction, and static plain16384 changes about+0.9% in this limited run. Public warm allocation is unchanged. A cache thrashing or disabled caller gets little measured CPU benefit and pays additional gross allocation; use the existing cache policy according to its workload. No public API, dependency, default limit, fixture or system-font policy is changed.

The standard54-case matrix uses the original static/emoji fixed fixtures, compares against `sbp9-selection-source-final`, validates all accepted seven-operation digests and unchanged default refusals, and measures separate cold/timing/memory scopes. It is a static control, not additional evidence for every variable font. No actual ital-axis performance, all-script/font distributions, parallel contention, browser/raikiri frame time or RSS is established by this diagnosis. Performance remains independent of the raikiri switching/S4 gate.

## Artifacts and reproduction

Compact manifest includes scope/iteration counts, signature SHA256 and representative instance, deterministic allocation counts, actual hit/miss counts, all72 ABBA medians and standard54 comparison. Raw archive, checksum in the manifest, retains every canonical/pilot sample, font and license, exact baseline/candidate engine and harness sources, workspace/probe locks, local overlay contents, commands, validation logs, source reconstruction and verification scripts. Baseline/candidate source fingerprints are independently reconstructed with the harness's Path-component ordering. Immutable executables stay in `target/performance-artifacts/sbp10-variable-probe` and are SHA-identified in the raw capture metadata.

Reproduction uses the archived `capture.py before|after`, `verify.py before|after`, `balanced.py`, `package.py` and final-proof script with the preserved fixed input and corresponding source snapshots. The local absolute paths identify the isolated workspace; adjust them consistently in a new checkout. `taskset -c 10 python3 tools/bench/run.py --output target/performance-artifacts/reproduced-sbp10 --baseline target/performance-artifacts/sbp9-selection-source-final --quick --cold-samples 2` runs the standard matrix against the prior54-case artifact in a checkout containing that baseline directory. The final verifier checks source/font/archive identity, all72 signatures and actual counters, all6 counter-free timing runs, unchanged54-case digests/refusals and required checks. The native ledger records scope/probe/selector decisions and their costs; no deferred minor is assumed before the independent review.
