# Font-data and metric reuse — shodo-sbp.12

Candidate A uses the FontData already acquired by shape_items, borrowing its bytes and parsing one skrifa FontRef for horizontal and vertical metrics. Public getters share the unchanged metric formulas. Candidate B independently constructs preliminary variation coordinates only when font-size-adjust needs them. Final variation/automatic opsz/explicit override/clamp order remains unchanged. No cache is added. Public APIs, dependencies, limits, font assets and saved spikes are unchanged. Performance never blocks raikiri switching or S4.

## Fixed inputs and independent comparisons

Baseline merged main `215006b9a7c0e2650d808d7456946e8718e75f00` has fingerprint `011040a1af1aa9789b27e4a2f9820f9d3dea2e71537fb64a9087efe90c2822ce`. Frozen A commit `02fc2fbb49324ba37d849b11b756ff971b41dc03` has fingerprint `c34c0c72b2ce0af35ab06e959752b093bf3c61fbbf9dd893ba359021047c4f14`. Final implementation commit `ad9217ad6412cfa93ef29e5e6014129b4cc28f50` has fingerprint `ac2859b145872b960b34ec31a98696da5eda6f4b70dde9a7413b4c4c92fb85c6`. Harness `7c139740f9da6bc5ab72420426c81ad5456569e36a99b18b2266fe392b18f80b` is unchanged. Archive exact source snapshots reconstruct all three fingerprints using Path-component ordering.

The45 measured public-build conditions combine fixed Latin/CJK/Roboto Flex, horizontal/upright/explicit wght-wdth-slnt-opsz/ex-height adjustment/ic-height adjustment, and64/1,1024/1,1024/8 chunk/style counts. Eight alternating sizes force many short runs. Real Roboto Flex has13 axes and enables automatic opsz; explicit opsz28 remains literal. Fixed font hashes, licenses and the pinned Roboto Flex upstream manifest are archived. No system discovery occurs.

Every condition preserves the complete processed text, DOM mapping at every byte offset, every glyph/line/run geometry, orientation, normalized/design variation coordinates, adjusted size, horizontal11/vertical3 metric bits, synthesis and warnings. All glyph IDs are nonzero. Each run's metrics also equal independent public getter values outside the measured scope. Six correctness-only generation controls retain the old paragraph, match and FontData across root registration and actual document-local family shadowing; new document face index0 remains distinct from the shared root stub. Generation controls make no allocation/time claim. Unit tests independently pin vertical MVAR deltas+0.4/+0.6/+0.2, the document index0 and root-stub literal metrics, invalid sizes, clamping and unknown faces. Existing horizontal MVAR and VVAR/ic-height adjustment literal tests remain.

Scope includes builder/text/style preparation and warmed public build; registered fonts, prewarm, line layout, reference metrics and signature validation are excluded. Paragraph remains held at scope end. Timing, allocator-only and trace-only use three separate immutable binaries for each state. The allocator is byte-identical to shodo-bench. Memory3 repeats are deterministic. Trace hooks remain in local archived overlays, with identical inline no-op forwarders in counter-free states; they are diagnostic observability, not proof that the optimizer preserves every counted operation.

## Operations and allocation

Trace order: all FontCollection::state Mutex locks; font_data leaf locks; FontData Blob Arc clones; shaper-cache-hit Arc clones; selected FontRef construction attempts; preliminary/final coordinate instance constructors; shape_items items. FontData derives Clone, and the archived dependency Blob::clone performs one Arc::clone. Arc counters exclude other Arc ownership; parser counters cover only shape/instance/font-metrics/line-metrics, excluding matching/check/registration and other parsers. This is not a one-parse-per-entire-run claim.

For Latin/variable horizontal1024/8 builds, baseline trace is `[10312,4136,4136,1024,5200,1048,1048,1024]`; A is `[8264,2088,2088,1024,4176,1048,1048,1024]`; AB is `[8264,2088,2088,1024,4176,0,1048,1024]`. A removes two data acquisitions/Blob clones and one selected parse per shaped item; shaper cache ownership is unchanged. B removes preliminary constructors only without size-adjust; both constructors remain when adjusting, including unsupported metrics that retain size and warnings.

| Scope | Calls before → A → AB | Gross bytes before → A → AB | Net bytes before → A → AB | Extra peak before → A → AB |
|---|---:|---:|---:|---:|
|build/0/horizontal/1024/8|22886 → 22886 → 22886|9174825 → 9174825 → 9174825|2187718 → 2187718 → 2187718|2741695 → 2741695 → 2741695|
|build/1/upright/1024/8|23934 → 23934 → 23934|8835017 → 8835017 → 8835017|2023862 → 2023862 → 2023862|2635183 → 2635183 → 2635183|
|build/2/horizontal/1024/8|28150 → 28150 → 27102|9345177 → 9345177 → 9311641|2263494 → 2263494 → 2263494|2817471 → 2817471 → 2817471|
|build/2/explicit/1024/8|29186 → 29186 → 28138|9378329 → 9378329 → 9344793|2263782 → 2263782 → 2263782|2817823 → 2817823 → 2817823|
|build/2/adjust-ex-height/1024/8|28150 → 28150 → 28150|9345177 → 9345177 → 9345177|2263494 → 2263494 → 2263494|2817471 → 2817471 → 2817471|
|build/0/horizontal/1024/1|3355 → 3355 → 3355|2907292 → 2907292 → 2907292|1079579 → 1079579 → 1079579|1087241 → 1087241 → 1087241|

A leaves calls/gross/net/peak unchanged in all45 cases. B removes1048 allocation calls and33536 gross bytes in the1024/8 variable-font horizontal example, with unchanged net/peak. Static no-adjust fonts reduce observed constructor calls but have no allocation benefit. Adjusted cases keep their allocation counts. No retention/cache/RSS reduction is claimed.

## Counter-free time and adoption

Rust1.97.1, release opt3/debug0/incremental false, CPU10. After local checks and the54-case standard control finish, each binary gets a warmup, then twelve fresh processes in before,A,AB,AB,A,before order repeated twice. Each process has10 samples per condition; all four per-state process medians and every raw sample remain, including outliers. Ratios compare means of process medians and carry no statistical-confidence or universal speed guarantee.

| Scope | Before process medians, ms | A process medians, ms | AB process medians, ms | A/before | AB/A |
|---|---|---|---|---:|---:|
|build/0/horizontal/1024/8|6.5248 / 6.5529 / 6.5396 / 6.5456|6.4126 / 6.3864 / 6.3990 / 6.4192|6.2656 / 6.3077 / 6.2937 / 6.2667|0.9791|0.9811|
|build/1/upright/1024/8|5.7315 / 5.7990 / 5.7755 / 5.7714|5.6455 / 5.6327 / 5.5798 / 5.6497|5.4435 / 5.4328 / 5.4895 / 5.4330|0.9753|0.9685|
|build/2/horizontal/1024/8|8.6049 / 8.6498 / 8.6703 / 8.6409|8.3476 / 8.4151 / 8.4480 / 8.5188|8.0925 / 7.9585 / 8.0194 / 8.0235|0.9758|0.9515|
|build/2/explicit/1024/8|9.3316 / 9.4697 / 9.4452 / 9.4224|9.1149 / 9.1757 / 9.2170 / 9.2783|8.5202 / 8.4496 / 8.4813 / 8.5101|0.9766|0.9232|
|build/2/adjust-ex-height/1024/8|8.8476 / 8.9188 / 8.7876 / 8.8855|8.5631 / 8.6563 / 8.6738 / 8.7193|8.6446 / 8.5621 / 8.6095 / 8.6086|0.9767|0.9946|
|build/0/horizontal/1024/1|2.0499 / 2.0423 / 2.0541 / 2.0346|2.1407 / 2.1847 / 2.1594 / 2.1495|2.0440 / 2.0565 / 2.0572 / 2.0404|1.0554|0.9495|

Adopt both bounded internal reductions: A removes repeated ownership acquisition and metric parsing; B removes unused preliminary coordinate construction and its measured variable-font allocation. The table reports each component separately. Adjusted controls also move despite retaining the same constructor/allocation counts (variable ic-height AB/A1.0032; CJK ex-height0.9679); their cause is unproven and may involve code generation or host variance, so timing ratios are observations rather than constructor-cost estimates. The single-style Latin horizontal1024/1 control regresses5.5% under A, while AB returns close to baseline; A is not a stand-alone CPU improvement for every input. CPU effects are scoped to this fixed-input host experiment, and small/noisy differences do not establish general speedups. Default limits and retained inputs remain unchanged. Cold font registration, all fonts/scripts, parallel contention, RSS and browser/raikiri frame time are unmeasured.

## Verification and reproduction

Actual A acquisition regression failed3 versus expected1, then passed1; B no-adjust constructor regression failed2 versus1, then passed1 while adjusted retains2. Focused shape25, metrics18 and earlier font/browser checks pass. Workspace1089, no-default636, complex-only639, fmt, all-target Clippy with denied warnings, docs with denied warnings and fixed snapshots pass. Setup syntax/API/private-test-module failures are retained as setup failures, not resource RED. Repository TMPDIR is used for local checks after the previous issue's linker storage failure; no source workaround is involved.

Standard54 conditions versus sbp11-single-pass-features preserve accepted output digests and default refusals. Initial capture/control timings may overlap compilation and are not CPU evidence; the final15 warmup/balanced processes follow all builds. [Manifest](data/font-data-reuse.json) and [raw archive](data/font-data-reuse-raw.json.gz) preserve all three engine snapshots, harness/locks/overlays, binary/font hashes, fonts/licenses, full outputs, samples, counters, setup/validation logs and reproducible Python/Rust probe code. The manifest checksum verifies the compressed archive. Reproduce capture.py before|a|ab using corresponding production snapshots, verify.py, balanced.py, summarize.py, package.py and final-proof; adjust absolute paths consistently. Local binaries remain in target/performance-artifacts/sbp12-font-probe. Integration/reviewer proofs are archived separately in sbp12-native-execution after branch finishing.
