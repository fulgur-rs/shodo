# Current core comparison and standalone baseline — 2026-10-11

This record completes shodo-xy2, the remaining measurement task under shodo-j2r.
It records current diagnostic measurements after the closed optimization tasks.
It establishes no performance release gate and adds no dependency to production cutover.

## Conditions and retained evidence

Both final runs use revision `3186adca27f89ac1bc1b9529a00315b6d2850e2b` (production engine unchanged from
`b703429`), Rust 1.97.1, release optimization 3, debug 0, incremental off,
complex-scripts enabled, and logical CPU affinity `[2]` on an AMD Ryzen 5 5600G.
The runners execute measured binaries sequentially; no other task build, tests or
measurement runs overlap the timing windows. CPU frequency and unrelated system
load are not controlled; these are single-run diagnostics, not regression thresholds.

- Engine source fingerprint v3: `b447d8e8dbf3cb77123f96950f16ffa258cb68a2538f55a3600390f08be44ff9`.
- Resolved Cargo.lock SHA256: `5be9b6e6002abb6e14ef666569861c54f7ba4809098dbd502816137188adb477`.
- Complete standalone report: `dev/bench/results/current.raw.json.gz`;
  uncompressed SHA256 `19158c769ca695818a621d3fba68f5c243e193b23b1da678b87b5bf68a10154a`.
- Standalone summary and three measured binary SHA256 values:
  `dev/bench/results/current.json`.
- Resolved dependencies: `dev/bench/results/current.Cargo.lock.gz`.
- Complete core samples and snapshots: `docs/records/core-comparison-2026-10-11.json.gz`;
  uncompressed SHA256 `b465016d22efbeeeca6517ae0b396303e0bdef47ae252b593f7d09fc018030e7`.
- Core measured binary SHA256: `d48be1c01adb9275cdd56a41314f5868b9b7922e8adb55600b65a9af6f8d54a5`.

The core runner fixes RUSTC to the stable compiler executable and explicitly
clears both compiler wrappers. The standalone runner uses its recorded inherited
Cargo configuration (including its cache wrapper); configuration hashes, effective
profiles, flags and actual toolchain are retained. The engines within the core
comparison share one executable and the same conditions. Standalone and core
harness numbers are separate observations with different setup boundaries.
Reproduction commands and baseline extraction are in
`docs/dev/performance-measurements.md`. Use the archived lock resolution in a
fresh checkout when reproducing dependency versions. Raw samples remain in the
formal compressed artifacts; redundant command logs, executable copies and
measurement directories are removed after validation.

A subsequent review fix guards Linux-only affinity and CPU metadata collection:
other platforms record null affinity and an empty CPU-model list when those
interfaces are absent. It changes the core runner's comparison hash, but does not
change the measured Rust example or timing windows. The archived measurements
retain the original runner hash and revision above; they were not regenerated.

## Core measurement and oracle

Shodo 0.0.25 and Parley 0.11.1 receive the 12 scale-1 fixture texts, fixed font bytes,
sizes and widths. Each fixture has three discarded warmups and 21 retained
samples per phase and engine, with alternating engine order. Font registration
and context construction are outside timing. Build includes style construction,
text analysis, shaping and paragraph/layout creation. Full line layout starts
from each freshly built paragraph/layout and includes Parley's Start alignment.
Snapshots, validation, destruction and JSON serialization are outside both windows.
There is no DOM, CSS cascade, box layout, rasterization or cold-process comparison.

Fixture language is supplied to both engines. Shodo explicitly uses fixture base
direction; Parley's public builder auto-detects it, and all 12 actual directions
match the fixture. Shodo rounds caller values to 1/64 px and rounds derived line
heights upward; Parley uses quantize=false. System discovery is disabled. Shodo
uses fixture family chains plus Latn/Hani/Arab fallback; Parley receives the
fixture chain followed by the three fixed families. Actual selected font hashes
and face indices are retained so fallback differences remain observable.

The oracle retains each engine's reference text, byte line ranges, native line
advance, line-local baseline/height, font identity and positioned glyphs. Shodo
RTL glyph origins are converted to physical x; glyph y is relative to each line's
baseline. Parley baseline is converted from paragraph coordinates to line-local
coordinates. Range equality is assessed only when reference strings match.
Native advances intentionally retain their API semantics: shodo excludes hanging
trailing spaces; Parley includes trailing whitespace. No numeric tolerance hides
geometry differences. Cluster mapping, painting and hit-testing are outside the
oracle, so it cannot establish complete engine output equivalence.

Eleven cases have identical reference text and line ranges. `latin-spacing` has
shodo whitespace collapse versus Parley's preserved input, so range equality is
recorded as null; both reference texts and actual ranges are retained. All 12
exact snapshots differ. For example, the first latin-short line has shodo height
21.796875 and baseline 17.109375 versus Parley height 21.79199981689453 and
baseline 17.104000091552734. Native advances also differ where trailing spaces
hang. These results do not justify equal-output speedup ratios. Historical
Latin-long/Arabic-long ratios are not cited as current evidence.

Warm medians in microseconds; each cell is shodo / Parley:

| Case | Build | Full line layout | Comparable ranges |
| --- | ---: | ---: | --- |
| latin-short | 54.9 / 24.2 | 21.0 / 2.4 | equal |
| latin-long | 445.3 / 191.3 | 429.9 / 9.0 | equal |
| latin-spacing | 37.3 / 15.7 | 19.4 / 2.0 | different text coordinates |
| latin-case-mapping | 41.4 / 15.9 | 13.3 / 1.9 | equal |
| japanese-short | 118.5 / 50.5 | 49.2 / 3.4 | equal |
| japanese-long | 578.7 / 339.7 | 305.2 / 13.8 | equal |
| japanese-supplementary | 49.0 / 17.1 | 28.6 / 2.0 | equal |
| arabic-short | 59.2 / 27.2 | 40.0 / 2.4 | equal |
| arabic-long | 457.6 / 199.8 | 913.4 / 7.1 | equal |
| combining-latin | 30.0 / 11.5 | 9.4 / 1.9 | equal |
| combining-arabic | 36.3 / 19.5 | 9.2 / 1.9 | equal |
| mixed-scripts | 136.1 / 68.5 | 58.7 / 3.7 | equal |

## Standalone baseline

The complete matrix has 54 workloads (12 corpus cases plus six structural
variants at scales 1/8/64), all seven warm operations, 3780 raw Criterion sample
pairs, 108 independent cold processes and 54 allocator-instrumented processes.
Every operation's deterministic digest and cold/warm/memory agreement pass the
strict report validator. Context initialization, font registration, build and
full layout are separate cold phases; parent process wall time includes startup,
JSON output and logging. Memory measures requested heap allocation and live/peak
ownership, not RSS. Normalization and all scopes remain in the complete report.

The old initial summary is retained as history and is incompatible with the
current fingerprint/harness/build conditions. This current report establishes a
new diagnostic baseline without a historical speedup comparison.

Selected warm medians in microseconds, at scales 1 / 8 / 64:

| Case | Build | All lines | Intrinsic |
| --- | ---: | ---: | ---: |
| latin-short | 47.3 / 266.1 / 2587.8 | 16.3 / 105.2 / 852.4 | 8.4 / 64.9 / 513.2 |
| japanese-short | 67.6 / 345.6 / 2738.8 | 18.9 / 152.0 / 1252.5 | 7.6 / 56.9 / 472.1 |
| arabic-short | 91.7 / 549.7 / 4166.2 | 31.5 / 328.7 / 3223.4 | 21.4 / 175.2 / 1081.7 |

## Measurement prerequisites and verification

The source fingerprint now covers embedded languages.dat as well as Rust sources
and manifests. Version 2 reports are rejected. A regression test reproduces the
missed language-data change before the fix and passes afterward. The probe's
--describe now returns the same 54 workloads as the timing harness: the previous
57 included three ad hoc itemize-graphemes inputs that have no Criterion series.
The itemize probes remain directly callable by ID/size. A matrix agreement test
reproduces 57 versus 54 before the fix and passes in cold and memory builds.
The rejected initial baseline was not adopted; only the final validated runs are
archived here. The first core run was superseded by the explicitly pinned-compiler run.

Verification: workspace tests 1755 passed, 0 failed, 9 ignored; workspace/all-target
Clippy with warnings denied; formatting; Python runner tests 27 passed; core
oracle tests 3 passed; probe tests 5 passed in cold and 6 in memory builds.
An independent read-only review verified coordinate/range handling, compiler
provenance, language-data coverage and the matrix fix.

C3 remains unchanged: do not defer or approximate edge reshaping to improve these
numbers. Range agreement alone is insufficient to justify removing shaping work.
Performance is diagnostic evidence, not a blocker for production migration.
