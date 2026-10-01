# Internal break_all previous-Line ownership

shodo-c91.1 removes the unused accepted-Line clone from the internal break_all driver, including grapheme limits. Public lines still gives callbacks an exact owned previous result. Both use the same private const-policy state machine; float/block/token/offset arithmetic and warnings are unchanged. No new cache, API, dependency or limit is added; S4/raikiri switching stays independent.

## Fixed-font full-output evidence

93 conditions × internal/public/manual drivers =279 rows:54 standard workloads,32 rich plain/first-line/Ruby/first-line-Ruby/forced/block/float/shared-edge cases, one real edge-window0/warnings1 fallback, three grapheme0/1/3 and NaN/negative/saturated-width controls. Existing pinned real Latin/CJK/Arabic fonts have system discovery disabled. Complete source mapping, full public glyph/run/cluster attributes, recursive Ruby/paint/geometry, tokens and ordered warnings match original/candidate across all six builds and four final processes. Manual next_line is an independent driver. Unit controls additionally exercise actual accumulated Q26 block-offset saturation with legal10million-px line height, float/block/forced transitions and suppression. Font-size alone is clamped at1million and did not trigger the required offset saturation in an initial control; its failure is preserved.

The real derived Line::clone is observed with a cfg(test) zero-sized marker: original internal ordinary/grapheme paths failed6/21 versus0, candidate passes0; public previous-Line copies and all fields remain intact. Shipping Line has no marker. Disposable trace replaces only the derived clone with field-equivalent clone plus fixed TLS counters, with readonly allocation deltas from the same allocator hooks. Counts include recursive child clones; top-level clone allocation spans include descendants once, not nested summed allocations. Three allocator samples are exactly neutral against trace for all operations/279rows. Trace ns are not adopted timing.

## Counter-free time and requested bytes

Four fresh CPU10 processes use ABBA states, seven samples plus two warmups per row. Each source has one forward and one reverse case/driver order. Builds/checks/standard54 and the main verifier finish before final processes. Input/paragraph/font/context preparation, full snapshots and serialization stay outside layout windows; output and context releases are separate. Earlier six prototype captures can overlap checks and are excluded from CPU conclusions. The median of the two process medians per state is reported without a confidence or universal speed claim. Validation/report storage contributes large outside-window process memory and allocator history; scoped net/peak are not total retention or RSS.

| Fixed case | Lines | Actual clones before→after | Clone gross before→after B | Layout gross before→after B | Layout net before→after B | Peak-extra before→after B | Before/after ms | after/before |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| rich/plain/32/80/default/width/internal | 96 | 96→0 | 22112→0 | 694284→672172 | 136064→136064 | 185216→185216 | 0.4884/0.4631 | 0.9483 |
| rich/ruby/32/80/default/width/internal | 40 | 88→0 | 56680→0 | 3846792→3790112 | 1253373→1253373 | 1277949→1277949 | 2.7160/2.6174 | 0.9637 |
| rich/first-line-ruby/32/80/default/width/internal | 41 | 89→0 | 56680→0 | 4984118→4927438 | 1253781→1253781 | 1278357→1278357 | 2.8146/2.8332 | 1.0066 |
| rich/edge/32/80/default/width/internal | 75 | 75→0 | 24159→0 | 2275069→2250910 | 267777→267777 | 331815→331159 | 1.9490/1.9319 | 0.9912 |
| standard/latin-long/64/internal | 1152 | 1152→0 | 703872→0 | 34174165→33470293 | 1602978→1602978 | 2389410→2389410 | 29.2195/29.0491 | 0.9942 |
| standard/arabic-long/64/internal | 769 | 769→0 | 1786809→0 | 92305226→90518417 | 2602472→2602472 | 2995688→2995688 | 77.6082/77.8077 | 1.0026 |

All279 timing rows and every raw sample/process median remain in the manifest/archive, including public/manual controls and adverse cases. Largest observed internal ratios: standard/mixed-scripts/8/internal 1.3935, standard/combining-arabic/8/internal 1.3934, standard/combining-arabic/64/internal 1.3876. Unchanged controls may move due to code layout/allocator/host variation; do not attribute every wall-time difference to clone removal.

Decision: adopt the private no-copy internal path for the independently verified removal of unnecessary deep clones and transient requested allocation. Accepted output/context ownership and their separate release scopes remain measured; no gross-to-retention inference or frame/parallel/cold-start speed guarantee.

## Verification and source identity

Core/resource/public callback controls, allocator/probe, workspace fmt/all-target Clippy/docs with warnings denied pass. Primary paired release binaries use rustc1.96.0; the standard runner separately uses stable1.97.1 as its fixed control. The current standard54 runner strictly matches the previous baseline conditions and all seven-operation digests/default controls. One initial comparison was rejected because documentation-only RUSTDOCFLAGS=-D warnings leaked into the benchmark environment; it is preserved, and the standardized rerun clears that variable. No baseline metadata was rewritten.

Measured original `8f3482b1c50f09728e6f058cda7de1a6a2b1d725`, candidate `0d070460795b9f7697bd59bca9299365af3ef120`. Measured engine hashes `0ed16ba57ea875f1dbdcff2d8b2c88ac3e65d8f052f6f7713a1d8d4a3c121aaf`→`fd7d141efa6b95f1bd9113bf818d56df2e379a502dada17478d8b8c0d025d7d1`; standard harness `7c139740f9da6bc5ab72420426c81ad5456569e36a99b18b2266fe392b18f80b`. Final repository hash `b886b1a5cf5bb051f4bbe547cdfb012974f3bebf3c379dcd179814ef5b9071ea` differs only by a test-only warnings-initializer lint fix in iter_tests.rs; verify.py proves exact normal source/example equality and the cfg(test) inclusion guard, rejecting any other difference. Both identities are retained rather than presenting the test-only source as measured.

Raw `internal-line-clone-raw.json.gz`: 1746846bytes, SHA256 `ff0c23ddf3e5aec755c83992e02d57e82fe8ad72eeee7e1a09a936396b302a0c`. Five SHA/length-pinned parts each smaller than50MB accompany the metadata archive and include all ten original report byte streams losslessly as gzip+base64 with SHA/length, complete measured core and custom source, unchanged harness, fonts/licenses, six immutable binary profiles/SHAs, overlays, recipes/logs, standard54 and final-process records. Decode report data base64 then gzip to restore exact JSON bytes. Measurement recipes are immutable; post-review/integration scripts stay outside the raw archive.

Reproduce with `cargo run --release -p shodo-bench --example internal_line_clone`; allocation-counting is separate. SHODO_CLONE_SAMPLES controls positive sample count and SHODO_CLONE_REVERSE reverses case/driver order. Archive capture.py, verify.py, balanced.py, package.py and final_proof.py pin source/build/font/output/table gates; adjust their absolute checkout paths explicitly. Final Astra whole-branch review and latest-head CI gate the PR merge.
