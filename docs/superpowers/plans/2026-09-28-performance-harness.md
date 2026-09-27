# Standalone performance harness implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Native inline execution selected under the existing autonomous issue workflow; only one whole-branch reviewer after full implementation.

**Goal:** Reproducible standalone shodo time/cold/allocator measurements with compatible baseline comparisons and raw evidence.

**Architecture:** A dev-only workload library feeds Criterion timing and a separate process probe. A Python runner preserves validated raw results, conditions and comparisons; an opt-in allocator feature never enters timing builds.

**Tech Stack:** Rust1.89+, pinned Criterion0.7.0, existing serde/serde_json/SHA256 and fixed shodo-fixtures, Python standard library.

**Spec:** docs/superpowers/specs/2026-09-28-performance-harness-design.md

## Global constraints

- Base635269db13d45e9d8a391fb7cf2cb854363e81e4, shodo-p2m.8, owned /home/mitz/Work/oss/shodo/target/worktrees/shodo-benchmark feat/performance-harness (relocated from /tmp/shodo-benchmark after shared tmp quota failure). .6 excluded; S4 untouched/unmerged.
- Rust1.89 floor; dev crate publish=false; root default members and normal library dependency graph unchanged.
- Release opt3/debug0/incrementalfalse; fixed complex-scripts feature, no web/system discovery; same fixed face bytes and source/settings hashes on every path.
- Criterion time excludes output drops and benchmark string/metadata setup; allocator binary separate feature/build; raw counters mean allocator-visible requested bytes, not RSS/native/stack memory.
- Preserve existing output on invalid config, failed command or invalid/incompatible data. Unknown unsupported/runtime outcomes abort. Fixed digest/counts exclude process-local IDs.
- One whole final independent review and one fix pass; all local gates plus exact PR HEAD check/msrv/wasm before merge, then close/cleanup and bd ready.

## Review focus

- Width-change reuse must produce the same literal source/glyph output as fresh builds, including tabs/float retries; test Task1.
- Height rejection must retain the original token and source; retries must terminate or fail explicitly; test Task1.
- Scope realloc failure, shrink and preexisting owner release must preserve actual live bytes and signed net; test Task2.
- Warm font/context state must not leak into process-cold or allocator-free timing; test Task2 with real child invocations/feature rejection.
- A stale/missing case or incompatible font/compiler/host metadata must fail before replacing prior results; test Task3.

### Task 1: Fixed workload library and public protocol

**Files:** create dev/bench/Cargo.toml, src/{lib,workload,digest}.rs, tests/workloads.rs; modify root Cargo.toml workspace members.

**Interfaces:** Workload {id:String,scale:usize,...fixed prepared style/text}, workloads()->Vec<Workload>; Workload::build(&mut LayoutContext,&FixtureFonts,&Limits)->Result<Vec<Paragraph>,BenchError>; layout(&Workload,&[Paragraph],&mut LayoutContext,Operation)->Result<Run,BenchError>; Operation {FirstLine,AllLines,Intrinsic,ReuseWidths,RebuildWidths,PageRetry}; Run {lines:Vec<Line>,float_reports:usize,height_retries:usize,intrinsics:Vec<IntrinsicSizes>}; digest(&Run,&FixtureFonts)->Digest with stable SHA256/check counts and fixed-face identity. build and intrinsic have explicit measured output types; layout errors never masquerade as Done.

- [x] Write public tests first: corpus IDs/scales and actual real-face glyph output; width reuse versus rebuild literal source/end/width evidence on preserved tab and float cases; forced height0 rejects and retry token unchanged; many-short counts1/8/64 not one concatenation; out-of-font/missing workload rejects; stable digest across separately loaded font collections; nested20x20atomic baseline16 and4nested padding2 affect actual geometry.
- [x] Run cargo +stable test --offline -p shodo-bench --test workloads; observe missing package/API RED before implementation, and retain later functional RED where needed.
- [x] Implement exact54workload matrix and APIs, fixed fallback, no system scanning, bounded protocol retries proportional to source units and hard cap; build returns errors. Digests computed outside measured scopes use stable font fixture index/hash, line ranges, dimensions, glyph cluster/id/position and actual counts.
- [x] Verify meaningful tests GREEN and full cargo test --offline --workspace; root dependency/default-members unchanged. Commit Task1.

### Task 2: Time, cold process and allocation probes

**Files:** create dev/bench/benches/layout.rs, src/allocator.rs, src/bin/probe.rs, tests/allocator.rs; modify package manifest features and bench target.

**Interfaces:** Criterion benchmark layout target harness=false consumes Workload APIs; CLI probe `--cold <id> <scale>` or `--memory <id> <scale>` emits validated JSON to stdout after scopes. AllocationCounts {calls,allocated_bytes,deallocated_bytes,live_bytes,peak_extra_bytes,net_bytes}; Scope::begin()->Result<Scope,...>, Scope::finish(self)->AllocationCounts; allocator feature only enabled in memory binary/tests. Probe outputs schema1,workload settings,operation/digest,count/scopes and durations (cold only).

- [x] Write raw allocator public-scope tests: alloc64→realloc128→shrink32→drop32 hand-derived calls3/gross224/freed224/net0/peak128; failed delegate realloc retains64; preexisting64drop givesnet-64 andpeak0; nested scope rejected; zeroed allocation observed; scope excludes later JSON/output allocations. Compile feature-targeted test separately and observe RED.
- [x] Write real process tests: timing build rejects --memory, instrumented build rejects --cold; process-cold output has full workload digest and init/register/build/layout phase fields with nonnegative durations; clocks never include process startup or metadata serialization; two child invocations cold each, no warm font preparation. Workload errors yield nonzero no valid partial JSON.
- [x] Implement delegated System allocator with nonallocating atomic callbacks and explicit unsafe contracts; no formatting/locks/unwind. Successful realloc count/gross/dealloc adjust live; failure no stats delta. Add memory checkpoints retaining and releasing actual fonts/context/paragraph/Line ownership, plain/justify delta, width change and page retries. Normalize only with nonzero glyph/Line denominators, report signed net.
- [x] Implement Criterion7operations for all selected cases. Prewarm warm font/context outside clocks, bounded LargeInput batching excludes setup/output drops; next_line retains actual Line, intrinsic real API, rebuild measured includes build. Environment filter limits measured workload set; quick sample10/warmup100ms/measurement200ms is explicitly identified. Fail runtime/digest mismatch. Cold inner phases exclude CLI/corpus prep, init includes actual new font/context and registration before first CJK shape; parent later reports process wall separately.
- [x] Verify workspace+feature allocator tests, real cold/memory probes at scales1/8/64 for CJK/mixed/structural cases, Criterion smoke `cargo test -p shodo-bench --benches`, no instrumented timing. Commit Task2.

### Task 3: Validated runner, baseline comparison, actual evidence and CI

**Files:** create dev/bench/tools/{run,test_run}.py, docs/performance-measurements.md, .github/workflows/performance.yml; modify .github/workflows/ci.yml; initial summary dev/bench/results/initial.json after actual run.

**Interfaces:** CLI `python3 dev/bench/tools/run.py --output <newdir> [--baseline <dir>] [--quick] [--case <id>] [--cold-samples <N>]`; validator validate_report(report:dict)->None, compare(current:dict,baseline:dict)->dict. output metadata.json, results.json, raw Criterion files, cold/memory JSON samples, copied Cargo.lock and subprocess logs. Comparison returns ratios/deltas by exact key; explicit mismatched conditions error before output publish.

- [x] Write unittest: changed font/settings/compiler/features/CPU rejects; missing/duplicate cases/digest mismatch/negative or NaN durations/counter inconsistency rejects; same valid machine permits revision difference and correct hand-derived time ratios/memory deltas; output remains unchanged when final child command fails or validation fails; no chosen cases rejects before writing/build; malformed samples no replacement. Execute real validator/comparator, command injection only for side-effect failure tests.
- [x] Observe unittest functional RED, implement argv-based orchestration with a staged sibling directory then atomic publish into new output directory. Validate config before commands; reject existing target without replacing it. Build package selected release timing binary without allocator, separate instrumented probe, preserve both executable paths before feature rebuild; Criterion output path explicitly owned per run. Collect machine/toolchain/build/source/input/font/lock metadata and successful child/raw samples; no self-comparison as empirical performance claim.
- [x] Run full actual quick matrix and repeated same-machine representative baseline measurement; save raw artifacts externally, small provenance-qualified initial summary and docs explaining costs/scopes/checksums/noise/limitations and commands. Retain historical docs/shaping-measurements.md.
- [x] Add normal CI smoke/accounting/Python checks, manual workflow_dispatch artifact performance run with no wall-time failure thresholds. Verify artifact paths and selected case/config are recorded.
- [ ] Run stable/MSRV full workspace, Clippy all-targets, docs warnings denied, fmt/diff, Python fixture+benchmark tests, generator/strict browser check, no-default/wasm relevant checks, complete actual measurement verification. Commit Task3. Dispatch one fresh whole-branch reviewer on immutable final HEAD; fix material findings once with RED/GREEN/full gates. Push/PR, exactHEAD CI success, merge/readback/containment, issue close/owned cleanup, bd ready.
