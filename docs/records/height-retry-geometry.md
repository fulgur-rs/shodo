# Remaining same-token height retry geometry

Issue `shodo-sbp.17`; base `12c7700e0296d801e4c7c7041916b2cd5e80c718`, measured example/source `4e9017b9bab30a064c5e603a8eee1159c9226243`.

The existing PartialLine cache already reuses the raw scan on clean same-width/token height retries. In the measured rich cases, additional retries do not increase actual shaper calls, but parent and Ruby annotation geometry still rebuilds on each rejection and acceptance. This remaining cost justifies a conditional private CompletedTrial/height-stage implementation candidate. This PR ships diagnostic evidence, not a cache or speedup. The existing scan-cache work and .1 per-line budget reset remain solved.

## Counter-free costs

195 conditions: all54 existing fixed-font workloads with unchanged public AllLines/PageRetry (108), plus7 rich kinds×1/32repetitions×80/240widths×0/1/4rejections (84) and plain edge-window0/warnings1×3 (3). Source/limits/fonts are identical within each comparison. Four final fresh CPU10 counter-free processes use7samples plus2warmups; the table is the median of process medians. Standard and rich section order stays fixed; reverse applies within sections and their operation/retry order, forward/reverse/reverse/forward. The initial four-process set overlapped a native verifier repeat and is preserved as controls, excluded from conclusions; final captures began after all builds/checks/verifier completed.

Rich repeat32/width80, whole operation milliseconds:

| Input | Direct (0) | Reject1 then accept | Reject4 then accept | 1/direct | 4/direct |
|---|---:|---:|---:|---:|---:|
| plain | 0.490 | 0.588 | 0.883 | 1.201 | 1.804 |
| first-line | 1.512 | 1.619 | 1.907 | 1.071 | 1.261 |
| ruby | 2.664 | 3.106 | 4.349 | 1.166 | 1.632 |
| first-line-ruby | 2.780 | 3.239 | 4.487 | 1.165 | 1.614 |
| forced | 0.438 | 0.549 | 0.855 | 1.253 | 1.951 |
| block | 0.440 | 0.547 | 0.837 | 1.244 | 1.903 |
| float | 0.570 | 0.689 | 1.008 | 1.209 | 1.768 |

Every standard PageRetry still rejects each actual line exactly once; its full accepted output, float reports and observed warnings equal fresh AllLines for these fixed workloads. All108 standard cases and87rich conditions retain individual time/alloc/phase samples. Results compare different numbers of public calls, not an implemented optimization. No universal/frame/parallel/RSS guarantee follows.

Input assembly, paragraph build, configured shared fonts, context initialization, full snapshots/fresh oracles and Auto-plan height controls are outside timed windows. Standard uses the original public workload::layout. The rich driver includes per-call warning drain/drop and bounded progress checks in its whole operation, while snapshots stay outside. Accepted output release and context release are separate windows.

## Actual scan reuse and geometry

The disposable observer counts scan::scan calls, the exact fast same-width cached.scan.clone return, initial safe raw clone retention, and per-line edge-spend resets. The same-width clone counter is branch-specific: it excludes the indexed retained-scan path used by some floats, and excludes annotation selected scans. Raw-scan counts and full outputs avoid mislabeling those paths as an unresolved cache failure. Unsafe/tab/warning paths may legitimately rescan. Actual shaper hooks count calls and real scalar/UTF8 inputs at both production sites.

The phase observer uses noalloc fixed arrays/stack and read-only snapshots of the same allocator; no nested begin scopes. Root and annotation scan, post-scan and materialization retain inclusive/exclusive ns and allocation calls/gross/freed. Derive signed net as gross minus freed; never add inclusive nested spans. Materialization covers whitespace/alignment/positions/Line/metrics/Ruby placement/warnings/height gate and rejected-Line destruction. Post-scan includes materialization; annotation(anydepth) is separately classified. Per-phase ns contain observer overhead and cannot predict savings; only whole-operation counter-free time compares current calls. Per-phase peak is unmeasured.

Rich repeat32/width80, reject4: observed median requested allocations and actual counts:

| Input | Lines | Fresh raw scans | Fast same-width clones | Root materializations | Annotation materializations | Shaper calls | Whole gross bytes | Exclusive materialization gross bytes | Accepted output release bytes |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| plain | 96 | 96 | 384 | 480 | 0 | 64 | 1362092 | 445440 | 90624 |
| first-line | 96 | 96 | 384 | 480 | 0 | 444 | 2374240 | 437160 | 95004 |
| ruby | 40 | 40 | 160 | 200 | 240 | 8 | 6289856 | 1966840 | 165920 |
| first-line-ruby | 41 | 41 | 164 | 205 | 240 | 8 | 7423350 | 1962530 | 165992 |
| forced | 96 | 96 | 384 | 480 | 0 | 64 | 1253548 | 445440 | 90624 |
| block | 96 | 96 | 384 | 480 | 0 | 64 | 1349164 | 445440 | 139776 |
| float | 96 | 96 | 256 | 480 | 0 | 64 | 1401196 | 448640 | 90752 |

Clean plain root geometry counts equal lines×(1+rejects); raw scans remain lines and fast same-width reuse equals lines×rejects. Ruby child geometry also repeats. Shaper calls stay equal across0/1/4 retries in the displayed rich conditions, so do not credit a new cache with already-achieved shaping reuse. Increases in materialization gross are measured; removing that entire phase is not automatically correct or an achieved speedup.

Requested samples (calls/gross/freed/net/whole-operation peak) are identical between standalone memory and observer builds, across every condition/operation. Cost net includes returned accepted output and context changes. Cost plus output release is incremental context growth from the operation, not total context memory; paragraphs/fonts/build contexts live outside the scope. Output release includes enclosing Run/LineResult Vecs and recursive owned geometry. It is aggregate current output ownership, not a measured one-entry CompletedTrial or clone cost. Whole-operation peaks/releases remain separate from net; RSS is unmeasured.

## Conditional reuse design

Investigate a private one-entry CompletedTrial that owns the exact completed Line after a height rejection, or a height-only stage only after proving its exact dependencies. A second rejection could compare the retained exact needed height; acceptance may take the cached Line and evict, avoiding a compulsory clone. A later query for the same token then recomputes normally, preserving pure public behavior. If acceptance keeps a cached copy, measure real Line::clone allocations and recursive owners before adoption. No API change, cache or height-only fast path is implemented here.

A key must include paragraph/token/data and font-owner identity/generation policy, token flags and first-line lane/remapping, sanitized width and inline/block offsets, options, atomicsgeneration+revision, matching plan identity/options/ends, exact float cursor and relevant placed/inset state, and immutable limits/budget policy plus eligible resource/cache state. Height can be excluded only when its sanitizer/warnings and final comparison are isolated without affecting completed geometry. Any key change, font/input replacement, atomics/plan change, cache shrink0 or invalidation evicts the entry.

Start conservatively with warning-free, unsuppressed, resource-fallback-free completed trials and proved compatible cache/budget state. For anything ineligible retain the original path. Height-dependent sanitization/warnings still execute on hits. Do not blindly replay spent budget from an earlier call: actual cache hits can change reshape spending. Preserve the original per-line resets, effective eligibility and warning sequence/caps/suppression across retries and subsequent continuation, not merely an isolated first line. Existing unsafe/tab/warning raw-scan fallbacks are intentional.

Retain at most one completed Line per LayoutContext. Proposed private initial policy:64KiB owned-geometry budget, charged for the Line header, owned Vec capacities/overlays and recursive Ruby records/child geometry. This cap is a design choice, not shipping or measured behavior. Shared immutable data/font owners are not extra cloned buffers, but their prolonged lifetime must be accounted separately; it is not a64KiB total-memory claim. Oversized/ineligible trials are not cached; replacement/eviction/shrink0 drops ownership. Independently measure actual single-entry retention, cached-to-accepted transfers, clone costs when used, and owner lifetimes/whole peak before shipping. The aggregate output release table is not a per-entry forecast.

Mandatory future gates: exact source/font/glyph/paint/geometry/needed height, fresh and continuation outputs; forced/block/float/withdrawal progress; real first-line/Ruby extents/metrics/alignment/overhang; max-graphemes and plan mismatch fallbacks; exact warnings and suppression plus height sanitizer; budget/resource/cache effects across sequences; bounded owner retention and invalidation. A height-only stage may not skip geometry-dependent metrics/Ruby or warnings until an exact equivalent is proved. Candidate before/after counter-free captures must demonstrate benefit after cache/clone bookkeeping and eligibility costs.

Decision: adopt the conditional private one-entry/height-stage work as an implementation candidate; defer shipping it until those gates and retention/benefit measurements pass. The issue AC is diagnosis plus conditional proposal and is completed by this evidence. No unimplemented speedup is reported, and S4/raikiri switching remains independent.

## Verification and archive

195 full original counter-free / allocator / observer outputs agree, including rich per-call warnings/needed height and recursive source mappings/Ruby/glyph/font/paint/geometry. Four final processes reproduce them. Standard PageRetry uses the original code and equals direct fresh accepted output for its fixed workloads. Rich each rejected/accepted call is compared with an independent fresh context; one/four reject counts equal accepted lines×retrycount, and Auto-plan height0/retry agrees with the direct first line outside windows. Literal guards validate real first-line32px, Ruby children, forced breaks, block counts and actual float cursor progress.

Existing cache/height/first-line/Ruby/float/limits groups, full core379passed2ignored, allocator tests, workspace fmt, all-target Clippy with warnings denied and docs with warnings denied pass. Core `0ed16ba57ea875f1dbdcff2d8b2c88ac3e65d8f052f6f7713a1d8d4a3c121aaf` and standard harness `7c139740f9da6bc5ab72420426c81ad5456569e36a99b18b2266fe392b18f80b` are unchanged; all54 standard workloads are nonetheless present in this diagnostic. Saved spikes and runtime/limits/Cargo/public APIs are unchanged. Latest PR CI remains required.

Custom source fingerprint: `4342897047a4e82de28bb1db9c6c15c362bbf9671df740d3b36faa8b94d1c842`.

Evidence: [manifest](data/height-retry-geometry.json), [raw archive](data/height-retry-geometry-raw.json.gz). Raw freezes source/fonts/licenses, exact allocator/phase/scan/shaper overlays and binary/build pins,195original/memory/observer captures,4final+4control processes, medians, checks, recipes and native progress. Full text/mapping are losslessly interned per output; repeated identical oracle outputs across captures use archive_output_ref and archived archive_codec.unpack_reports restores exact decoded reports. Final proof checks restored reports against original capture JSON and recalculates all summaries. The699MB initial uninterned prototype stays in root artifacts with its source/byte/SHA/equality proof; the archive excludes that redundant initial payload. Failed private-module import/single-element loop/observer-reset build logs are retained.

Reproduce with `cargo run --release -p shodo-bench --example height_geometry`; add `--features allocation-counting` for allocation windows. `SHODO_HEIGHT_SAMPLES` must be positive; `SHODO_HEIGHT_REVERSE` reverses within-section workload/operation/retry order. Original absolute paths and CPU10 are pinned in raw recipes; relocate explicitly on a different host. The verifier/codec/final proof reconstruct full accepted outputs, allocations and medians; do not equate a different-host result to the archived measurements.
