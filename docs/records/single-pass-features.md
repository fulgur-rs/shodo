# Single-pass font-run features — shodo-sbp.11

Coordinate/size resolution previously built `features(style)`, then shaping replaced it with `for_item(style, item)`. The resolver's two metric consumers only read coordinates and size. It now leaves features empty; shaping remains the owner of the final features, including missing fonts. Upright shaping appends CSS and author features directly to its existing orientation Vec instead of extending from another temporary Vec. Public APIs, dependencies, default limits, fixture assets and saved spikes are unchanged.

Exact feature order remains automatic width, vertical defaults/vkrn, CSS components, explicit author settings last. Duplicate author tags remain ordered. Horizontal, sideways and combined runs omit automatic vertical defaults. No variation, size-adjust or metric behavior changes.

## Fixed inputs and measurement

Baseline is merged main `d3b9a1ac7f72a7df7102898eb73deed31d111588`, source fingerprint `5fb9d61191ba012385766c0a6605c43f5bd9a606ecacca576141875972c16a2f`. Candidate code is `b10da650130b895831612936e47139859905b36b`, fingerprint `011040a1af1aa9789b27e4a2f9820f9d3dea2e71537fb64a9087efe90c2822ce`. Standard harness remains `7c139740f9da6bc5ab72420426c81ad5456569e36a99b18b2266fe392b18f80b`.

The48 public-build conditions use unchanged pinned Latin/CJK fixtures, system discovery disabled, default versus kerning/ligature/duplicate-author-feature-rich styles, horizontal/upright/2-digit/4-digit combined text, and64 or1024 chunks with1 or8 styles. Alternating sizes force distinct short runs. Combined chunks remain separate inline boxes even with one style. Full raw signatures retain processed text, every source-mapping offset, every glyph/line/run position and advance, font/instance values, orientation and warnings. All48 full payloads are equal before/after and every sampled output equals its reference; all glyph IDs are nonzero.

Measured scopes include builder, text/style preparation and warm public build, with the paragraph retained until scope end. Registered fonts, warmed LayoutContext, prewarm, line layout and validation are outside. Absolute start/live bytes include retained outside-scope JSON data; compare allocation calls/gross/net/extra peak. Three memory repeats are deterministic. Counter-free time, allocator counting and actual style/item/upright/width construction tracing use three separate immutable binaries. The allocator is copied byte-identically from shodo-bench. Diagnostic hooks live only in archived local overlays; no product entry point ships.

Existing fixed fonts selected zero automatic width features. The2/4-digit combined controls verify public output, while literal private tests pin automatic hwid/twid/qwid prefix order and explicit overrides. Automatic-width font performance is unmeasured. This distinction prevents claiming coverage the real-font probe did not execute.

## Allocations and retained data

|1024 chunks,8 styles unless specified | Calls before → after | Gross bytes before → after | Net bytes before → after | Extra peak before → after |
|---|---:|---:|---:|---:|
| Rich Latin horizontal |30138 →26994|10161097 →9691593|2450366 →2450366|3004455 →3004455|
| Rich CJK upright |33258 →28042|10136169 →9351785|2237358 →2253742|2848791 →2865175|
| Rich CJK combined2 |32246 →27030|9807305 →9022921|2363326 →2379710|2968615 →2984999|
| Default Latin horizontal,1 style |3355 →3355|2907292 →2907292|1079579 →1079579|1087241 →1087241|
| Default CJK upright |23934 →23934|8835017 →8835017|2023862 →2023862|2635183 →2635183|

Rich Latin horizontal style-feature constructions fall2072→1024 for1024 shaped items; rich CJK upright2080→1024. Candidate style construction equals item construction in all48 cases. Default horizontal1-style falls5→1 constructions but has no allocation difference: its empty style Vec never allocated.

Rich upright/combined cases retain an additional16384 bytes per1024 retained items: direct pushes grow the final Vec to a different capacity from the former extend operation. This is a measured capacity tradeoff, not a net-memory reduction. Rich CJK upright reduces gross784384 bytes and5216 allocation calls while retained net rises about0.73%. Default allocation scopes are unchanged. Full48 results are in the manifest.

## Time and adoption

Rust1.97.1 release opt3/debug0/incremental false, CPU10, fixed inputs and identical harness. After local builds and the standard control capture finish, each immutable timing binary gets a warmup, then ABBA four fresh processes. A localized CJK timing excursion appears in the second candidate process. All original samples remain; an opposite-order BAAB replication adds four fresh processes. Each process has10 samples per condition. Report all eight process medians; no outlier is removed and no statistical confidence is inferred.

| Scope | Before process medians, ms | After process medians, ms |
|---|---|---|
| Default Latin horizontal1024/1 |2.0584 /2.0752 /2.0512 /2.0662|2.0534 /2.0782 /2.0489 /2.0402|
| Rich Latin horizontal1024/8 |7.5575 /7.5374 /7.6364 /7.5946|7.4200 /7.4602 /7.4306 /7.4162|
| Rich Latin upright1024/8 |7.9748 /7.8024 /7.8799 /7.8589|7.5369 /7.7245 /7.5830 /7.5788|
| Rich CJK horizontal1024/8 |6.7131 /6.7784 /6.6945 /6.7095|6.6723 /12.0020 /6.5257 /6.4901|
| Rich CJK upright1024/8 |6.9596 /6.8679 /6.9124 /6.8986|6.6924 /12.3862 /6.6374 /6.6569|
| Rich CJK combined2 1024/8 |6.3863 /6.3747 /6.3929 /6.3854|6.1615 /11.3166 /6.1226 /6.1333|

Adopt the private refactor for its deterministic reduction in duplicate construction and gross allocation. Rich Latin horizontal mean-of-process-medians falls about2.0%, upright about3.5% in this host experiment. CJK horizontal/upright/combined2 averages including the excursion rise17.8%/17.1%/16.4%; other candidate processes are lower, but the cause of the excursion is unproven. These measurements do not establish a stable CJK CPU improvement or a universal speedup. Default allocation benefits are absent and default time is close to unchanged in the Latin long-run control. Retained capacity overhead remains explicit. RSS, parallel contention, all fonts/scripts and browser/raikiri frame time are unmeasured. Performance does not block raikiri switching or S4.

## Verification and artifacts

The coordinate-only resource test failed with actual1 feature construction versus expected0, then passed with0. Ordered feature tests cover all five orientations, width prefixes, duplicate overrides, global feature ranges, vertical kerning and explicit vrt2. Shape24 (including tiny shared shaping windows), metric15, workspace1085, no-default632, complex-only635, fmt, all-target Clippy with denied warnings, docs with denied warnings and fixed snapshots pass. One workspace doctest initially failed because its /tmp linker received SIGBUS; a sandbox launch also reported temporary mount quota. The failure logs are retained. Repeating remaining checks with a dedicated repository TMPDIR passed without source changes; temporary-storage pressure is an inference, not a proven linker defect.

The standard54-case control versus `sbp10-variable-matches` preserves all accepted seven-operation digests and default refusals. Initial probe/control timings can overlap local build work and are not adopted CPU evidence; the ten final warmup/ABBA/BAAB processes follow local builds. Manifest and raw archive retain all48 full output payloads, all samples (including the excursion), source/overlay/fixture/lock/binary hashes, exact engine/harness code, fonts/licenses, commands and check logs. The manifest verifies the compressed archive checksum; source fingerprints are reconstructed with the standard Path-component ordering.

Reproduce with archived capture.py before|after, verify.py before|after, balanced.py, repeat.py, summarize.py, package.py and the final-proof script using the corresponding production snapshots and fixed fixtures. Adjust archived absolute paths consistently. Standard control command: `taskset -c 10 python3 tools/bench/run.py --output target/performance-artifacts/reproduced-sbp11 --baseline target/performance-artifacts/sbp10-variable-matches --quick --cold-samples 2`. Immutable local binaries are under target/performance-artifacts/sbp11-feature-probe and SHA-identified. Final reviewer findings and integration proofs are preserved separately in the native ledger archive after branch finishing.
