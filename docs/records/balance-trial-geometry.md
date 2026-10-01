# Bounded Balance trial geometry diagnosis

Issue: `shodo-sbp.16`. Base `081d738066daf4bf951f5e0ae1e158b628e0b9ec`; measured example/source `4bdd6b7be307311a6c3988768838a83ce7cd9e7e`.

Balance repeatedly constructs parent and Ruby annotation lines during its bounded width search. The measured cost justifies investigating a private count/end trial path. This PR adds the diagnostic and evidence; it does not implement that path or claim a speedup. Pretty already evaluates its additional candidates through selected scans and does not construct a full parent Line per candidate. Keep that distinction.

## Counter-free measurements

Four fresh CPU10 processes, forward/reverse/reverse/forward case and mode order, seven samples per condition and two warmups. All builds and full checks finished before these captures. The table uses the median of four process medians. Start means default Auto wrap and Start alignment, not a nonexistent Start wrap enum. Modes have identical source, fixed fonts and limits; different planned cuts are allowed.

32 repetitions, width80; plan and accepted windows in milliseconds:

| Input | Start plan | Balance plan | Pretty plan | Balance/Start | Start accepted | Balance accepted | Pretty accepted |
|---|---:|---:|---:|---:|---:|---:|---:|
| plain | 0.502 | 5.454 | 0.956 | 10.860 | 0.194 | 0.190 | 0.195 |
| first-line | 1.531 | 21.105 | 2.118 | 13.788 | 0.310 | 0.313 | 0.200 |
| ruby | 2.698 | 30.339 | 3.307 | 11.244 | 0.605 | 0.600 | 0.601 |
| first-line-ruby | 2.842 | 35.440 | 3.835 | 12.472 | 0.975 | 0.969 | 0.972 |
| forced | 0.468 | 4.619 | 0.936 | 9.874 | 0.198 | 0.198 | 0.199 |
| block | 0.460 | 4.563 | 0.916 | 9.929 | 0.198 | 0.195 | 0.199 |
| float | 0.585 | 6.525 | 1.031 | 11.158 | 0.216 | 0.214 | 0.219 |

The full 93-condition matrix also retains repeat1, width240, disabled planning, one-iteration/window1, and edge-window0/warnings1 controls. These are different algorithms, not before/after optimization ratios. Width searching is bounded by the default 16 iterations; the recorded binary-search sequence uses Q26.6 widths and exact measured trial line counts. Do not call it unrestricted quadratic search.

Input assembly, LayoutContext creation, shared configured fonts, opaque float-cursor discovery, snapshots and JSON serialization are outside measured windows. Build consumes the prepared builder; accepted uses the public lines iterator and includes its Line clones. Plan, accepted output, height controls and every owner release are separate windows. This is not end-to-end or frame time.

## Actual trial and geometry evidence

The disposable observer has a fixed stack, no allocated event buffers, actual shaper hooks at both production sites and read-only snapshots of the same global allocator counters. Whole-operation allocation scopes begin once. Root and annotation spans retain both inclusive and exclusive ns/gross/freed; derive signed net as gross minus freed. Never sum inclusive nested spans. Annotation means any nested depth, and its full recursive accepted output remains archived. Materialization covers whitespace/alignment/positions/Line creation/metrics/Ruby placement/warning/height gate; essential scan and pre-materialization preparation are separate. Rejected-Line destruction lies inside materialization for height controls. Public iterator clones and trial owner releases remain in whole-plan residuals.

The following are observer-build medians, not counter-free time or predicted savings. Balance, repeat32/width80. Materialization gross sums exclusive parent and annotation allocations, across greedy and trial roles:

| Input | Trials | Greedy parent Lines | All parent materializations | Annotation materializations | Actual shaper calls | Plan gross bytes | Exclusive materialization gross bytes | Materialization/plan gross |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| plain | 13 | 96 | 1632 | 0 | 190 | 7613048 | 1408768 | 0.185 |
| first-line | 12 | 96 | 1381 | 0 | 6419 | 19749891 | 1300524 | 0.066 |
| ruby | 12 | 40 | 589 | 615 | 124 | 30947543 | 5241108 | 0.169 |
| first-line-ruby | 12 | 41 | 601 | 621 | 130 | 63436204 | 5279985 | 0.083 |
| forced | 13 | 96 | 1632 | 0 | 190 | 5531781 | 1090048 | 0.197 |
| block | 13 | 96 | 1632 | 0 | 190 | 5508485 | 1090048 | 0.198 |
| float | 13 | 96 | 1632 | 0 | 190 | 8063032 | 1440768 | 0.179 |

The manifest records every phase ns, allocation calls, gross/freed/net and actual shaping scalar/UTF8 byte counts for all conditions. Phase timing includes observer overhead and is diagnostic only. Materialization alone is a subset of planning: removing all post-scan work would also remove necessary reshape/Ruby/spacing contracts. First-line shaping remains substantial even if Line materialization is reduced. Pretty creates greedy parent geometry but its extra selected candidates are a distinct cost.

Requested allocator samples (calls/gross/freed/net/whole-operation peak) are identical between the memory and observer builds, in every sampled operation. Per-phase peak is not measured. Net plan retention includes BreakPlan plus LayoutContext cache changes; it is not the ends Vec size. Builder/context/fonts have owners created outside build; build drops the builder. Summing net windows gives incremental owner changes, not total retained bytes. Release and whole-operation peaks are retained separately. RSS, production concurrency and parallel scaling are unmeasured.

## Conditional count/end proposal

Investigate a private typed `TrialEnd`/`TrialCount` return, initially only for Balance. Keep exact scan/selected edge viability, per-line reshape spend/reset, prepared overlays, Ruby base/annotation width/deficits, spacing and punctuation. Avoid only geometry proven unnecessary for count/end; do not proportionally split ligature advances, omit renderability predicates or skip the full post-scan phase. Compute any metrics/block extent needed for the iterator’s accumulated block offset exactly, including first-line and Ruby extents, because offset and saturation/warnings remain observable. Do not assume annotation Line geometry is removable until those dependencies are proved.

Required acceptance gates for any later implementation:

- Preserve paragraph/data/font ownership and normal/alternate first-line cursor remapping, FIRST_LINE/AFTER_FORCED flags, selected source ranges and real first-line styles.
- Preserve Ruby spans, visibility, child metrics/height/overhang/alignment, parent deficits and exact recursive retained source/glyph/geometry.
- Preserve forced and block boundaries, progress, FloatCursor discovery/withdrawal/displacements and accumulated block offsets. Floats remain zero-width anchors during plan search; accepted plan matching still requires the handled cursor.
- Preserve the same default iteration/window limits and width sequence, atomics generation/revision, width/options/para plan key, max-graphemes and mismatch fallbacks, and per-line edge budget resets.
- Preserve warning order/content, cap plus suppression sentinel, cache replay and resource fallback, including disabled/one-iteration/zero-edge-window controls. Geometry-only warnings are still observable; omission needs an exact equivalent warning path, not silent deletion.
- Fresh accepted geometry, same-token height rejection/retry, nested Ruby and both font shaper sites must remain exact. Count trials must not retain materialized Lines or introduce unbounded persistent caches.
- Measure gross/freed/net retention and whole-operation peak separately. Ship only after counter-free before/after captures demonstrate actual benefit; the measured phase cost is not promised savings.

The proposal is adopted as a future implementation candidate; the bypass itself is deferred pending these gates. No blanket Pretty rewrite is justified by this diagnosis. The issue’s acceptance criterion is measurement plus a conditional proposal, which this record completes. This work does not block S4/raikiri switching or modify saved spikes.

## Verification and reproduction

All 93 conditions have exact original counter-free / memory / observer plan Debug, full source mapping (including generated entries), glyph/font/paint/geometry and warnings. Opaque paragraph IDs are normalized only in source-pinned token/plan Debug, retaining unit/flags/options/ends and owner validity checked by public APIs. Four final counter-free processes reproduce those full outputs. The fresh planned oracle is universal; direct fresh break_all also matches Start on non-float inputs. Float cursors are discovered outside windows using an independent context. Height0 rejection then the same-token unlimited retry matches the first accepted full snapshot on every non-block/non-float condition. Literal controls confirm Ruby children, first-line32px, forced breaks, blocks and displaced future floats.

Existing plan/ruby/first-line/cache/float tests and all core lib tests pass: 379 passed,2 ignored. Bench allocator tests, workspace fmt, all-target Clippy with warnings denied, and workspace docs with warnings denied pass. Standard54 is not rerun: core `0ed16ba57ea875f1dbdcff2d8b2c88ac3e65d8f052f6f7713a1d8d4a3c121aaf` and standard harness `7c139740f9da6bc5ab72420426c81ad5456569e36a99b18b2266fe392b18f80b` are unchanged. The auto-discovered example and its snapshot are included in the separate custom fingerprint. Latest PR CI is required before merge.

Custom source fingerprint: `c6d9ba4fd20e26288bd85bbb9274ba1a35121b042b7c2269fbeb84eab909986a`.

Evidence: [manifest](data/balance-trial-geometry.json), [raw archive](data/balance-trial-geometry-raw.json.gz). The archive includes frozen source, fixed font bytes/checksums/licenses, exact disposable overlays, binary checksums/build commands, original and final captures, four balanced process provenance records, failed missing-browser-copy build log, passing checks and reproducible Python/Rust recipes. Binaries are rebuilt from those pinned sources; local immutable binaries remain in root `target/performance-artifacts/sbp16-balance-geometry`.

Run `cargo run --release -p shodo-bench --example plan_geometry` for counter-free diagnostics, or add `--features allocation-counting` for standalone allocation windows. `SHODO_PLAN_SAMPLES` controls positive sample count and `SHODO_PLAN_REVERSE` reverses input/mode order. Raw recipes preserve original absolute checkout paths, CPU10 and source fingerprint checks; relocate paths explicitly on another host and do not present the new host as the archived run.
