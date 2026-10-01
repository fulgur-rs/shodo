# Shared ligature prefix/suffix and continuation diagnosis

shodo-sbp.18 completes the fixed-real-font diagnostic. Shipping core, public output, limits and the standard harness are unchanged. Initial build prefix/suffix queries have no repeated observed signature; retain both real prefix widths and suffix renderability. Continuation queries show repeated work, so a bounded exact-width memo is a conditional future candidate. No runtime cache or speedup is implemented or claimed, and performance remains independent of the S4/raikiri switch.

The long cases use original **Material Icons** bytes at upstream [`bd8cb85`](https://github.com/google/material-design-icons/commit/bd8cb85bd4bad964fe6918f79665bb40c3a8efef), SHA-256 `ef149f08bdd2ff09a4e2c8573476b7b0f3fbb15b623954ade59899e7175bedda` (356840 bytes). Its [official guide](https://developers.google.com/fonts/docs/material_icons) describes word ligatures; the [pinned Apache-2.0 license](https://github.com/google/material-design-icons/blob/bd8cb85bd4bad964fe6918f79665bb40c3a8efef/LICENSE) is included beside the development example. Actual normal wide-line output verifies one nonmissing glyph per word for Noto Latin `fi`/`ffi`, `settings_input_component` (24 characters) and `signal_cellular_connected_no_internet_4_bar` (43). These are icon-font word ligatures, not evidence that ordinary body text is a generic hotspot. System font discovery is disabled; fixed fixture Latin/CJK/Arabic registration and asset bytes are archived.

## Contracts and measurement

86 public conditions: four words × 1/8 contiguous repeats × widths2/8/24/48/1024 × Normal/BreakAll =80 default cases, plus fi/icon24/icon43 with window0 or glyph1 at width8 =6 fallback controls. Real original single-glyph shared clusters expose exactly B−1 initial boundaries in default BreakAll. All original, allocator and observed outputs match complete source mapping, font bytes/IDs, glyphs/clusters/origins, bit-exact positions/advances/transforms/metrics and all public line/token fields. Fresh per-line contexts and separately rebuilt paragraphs match warning sequences and output; height0 rejection and same-token acceptance agree. Every line advances contiguous source coverage to its exact final byte. Normal versus BreakAll intentionally has different output; no optimization ratio is inferred between them.

Time uses four fresh CPU10 counter-free processes with seven samples and two warmups per case, entire-case ABBA order; displayed values are median of process medians. Builds, full checks and native task2 verifier ended before these final processes. Fixed fonts/input/ParagraphBuilder/context creation, oracle/fresh/height controls and snapshots are outside measured windows. Build measures `builder.build`, including required slice initialization. Layout measures public `next_line` calls plus result retention, warning drain/drop and bounded progress, without warning serialization. Releases of accepted output, layout context, paragraph and build context are separate. This is development CPU cost, not frame/end-to-end latency.

The disposable observer captures both actual `shaper.shape` sites. It stores exact central scalars/UTF-8 text and source offsets, pre/post context separately, font layer/index, style/level/size/script identity, output glyph count, narrow shaper-call ns and requested allocation. Category1/2 are initial prefix/suffix and3 is shared-cluster continuation;0 includes initial/other edge/materialization. Disjoint whole-window scopes also capture input preparation/font/buffer/plan/GlyphStore allocation, unlike narrow shaper events. Fixed TLS arrays allocate nothing inside windows; capacity overflow aborts, never drops events. A smaller observer failed before final capture; logs and overlays are retained. All three allocator samples per operation/condition are exactly neutral for calls/gross/freed/net/peak. Observer time includes overhead and is not a savings forecast. Tables use the first observed allocation sample; all raw samples/events are retained.

## Fixed default results

At font size24, width8, one word:

| Word | B | BreakAll lines | Normal build ms | BreakAll build ms | Normal layout ms | BreakAll layout ms | Initial prefix+suffix calls / central bytes | Layout shaper calls / central bytes / context bytes | Repeated observed layout signatures |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| fi | 2 | 2 | 0.016 | 0.021 | 0.004 | 0.022 | 2 / 2 | 6 / 6 / 6 | 4 |
| ffi | 3 | 3 | 0.025 | 0.033 | 0.005 | 0.055 | 4 / 6 | 14 / 16 / 26 | 10 |
| icon24 | 24 | 24 | 0.022 | 0.315 | 0.003 | 2.988 | 46 / 552 | 644 / 4692 / 5250 | 367 |
| icon43 | 43 | 43 | 0.027 | 1.154 | 0.003 | 17.885 | 84 / 1806 | 1974 / 26656 / 17600 | 1070 |

For each complete default single-glyph ASCII cluster, initial prefixes have lengths1…B−1 and suffixes B−1…1, so their **observed central input** sums to B(B−1), with two real calls per boundary. The initial whole-word call and context bytes are additional, explicit raw events. At B43 this is84 initial prefix/suffix calls and1806 central bytes; at B2 it is2 calls and2 bytes. B24/43 is inside the unchanged4096-byte window. This is bounded concrete input amplification, not an unbounded-input complexity or generic application claim.

The layout totals include continuation and other real edge shapes, not just the initial slice algorithm. Repeated observed signatures include site, central text, scalar offsets, pre/post context and recorded font/style identity. They are **not a proven general memo key**: language, features/variations/orientation and resource/warning/saturation eligibility still need proof. Shared continuation bypasses the existing `EdgeShapeCache` cached-window path; the already-working cache and xy2 cursor improvements are not relabelled unresolved.

Requested memory for default BreakAll width8; gross/peak are whole operation, released output includes its enclosing result Vec and all accepted Line geometry:

| Case | Build gross / peak bytes | Layout gross / freed / net / peak bytes | Narrow actual shaper gross bytes (build/layout) | Released accepted-output bytes | Incremental layout context after output release bytes |
|---|---:|---:|---:|---:|---:|
| fi×1 | 14790 / 6426 | 27697 / 21379 / 6318 / 7991 | 0 / 0 | 2888 | 3430 |
| ffi×1 | 17199 / 6631 | 46096 / 37480 / 8616 / 10289 | 0 / 0 | 3564 | 5052 |
| icon24×1 | 134059 / 12131 | 1497311 / 1432025 / 65286 / 66959 | 2048 / 0 | 28512 | 36774 |
| icon43×1 | 472802 / 18330 | 7260977 / 7125212 / 135765 / 137438 | 9536 / 38912 | 53644 | 82121 |
| icon43×8 | 3721338 / 112935 | 61616400 / 60887739 / 728661 / 730334 | 73152 / 435840 | 478688 | 249973 |

For icon43×8 at width8, the current layout has344 lines, 15801 actual shaper calls, 263825 central and154400 context bytes, 8275 repeated observed signatures; counter-free build/layout medians are9.197/150.667ms. All80 default and6 fallback rows, not only this case, are in the manifest. Whole-window preparation/store allocations can dominate even when narrow shaper-call gross is zero. Peak is whole-operation only; net/freed and output owners are separate. RSS, instrumentation TLS/BSS and single proposed-entry retention are not measured. Cost+output release is incremental operation context growth, not total context/font/input retention or cache-entry cost.

## Decision and future gates

**Defer initial prefix/suffix reuse.** The complete default build queries have no repeated observed offset/context signature. Prefix widths cannot be proportionally distributed, and suffix renderability protects glyph-budget fallbacks. A proposed algorithm removing either check lacks a measured equivalent; keep the original path.

**Adopt a conditional continuation-width investigation.** Start with a private per-public-line-call fixed8-entry memo of exact Q26 widths for successful, warning-free, unsaturated queries only. Key immutable ParagraphData/normal-or-firstline lane/shared owner, exact source scalar range and line-start/token flags, original itemization/font/style/bidi/script/lang/orientation/features/coords/variations plus pre/post context, and caller limits/budget/state dependencies. The observed signature alone is insufficient. Warning-producing/None/saturated/fallback queries retain the original path; do not replay a stale success or silently suppress warnings. Existing request charging, per-line resets and cached-window policy remain unchanged on hits. No suffix validation or proportional glyph split is removed.

Proposed storage is at most8 inline entries and initially a strict1KiB incremental bound per invocation (key fields must fit or fall back), discarded at call return or key change. Do not retain GlyphStore/FontData or build a table of every substring. These are future design constraints, not an implemented memory bound or measured speedup. Measure actual entry/header/key cost and recursive invocation/owner lifetime, and capture baseline/candidate counter-free time/gross/net/peak before shipping. Exact continuation/fresh geometry, warnings/caps/suppression, saturation and all resource fallbacks must match; broaden only after first-line/Ruby/float/forced/plan/atomics/font invalidation tests and the54 standard workloads. Saved spikes remain unchanged and the switch has no dependency on this proposal.

## Reproduction and verification

`cargo run --release -p shodo-bench --example ligature_edges` uses only checked-in byte-pinned fonts. `--features allocation-counting` enables the separate requested allocator; `SHODO_LIGATURE_SAMPLES=7` and `SHODO_LIGATURE_REVERSE=1` control samples/order. Cargo/std fixture sources are unchanged; this autoexample and custom snapshot/asset pins have an independent source identity. Disposable observer source/overlays and immutable binary SHA-256/profile/recipes are in the raw archive, not shipping core.

All captures preceding the Paragraph-owned build-warning correction are retained locally as historical controls with pinned SHA/length and excluded from final measurements; final three binaries and all four timing processes were recaptured. The fallback guards inspect actual Paragraph warnings as well as the layout context.

The raw gzip JSON contains every final original JSON report as lossless gzip+base64 bytes with SHA/length, all source, full fonts/licenses/catalog/provenance, three build overlays, all observed inputs/events, four final captures, recipes and logs. Decode report base64 then gzip to recover the exact original JSON bytes. Archived `verify.py` and `final_proof.py` independently check source@measured commit/current tree, binary/font pins, all output/height/warning/allocator identities, actual shared clusters and B(B−1) inputs, all final process medians and complete archive readback. Existing suffix-renderability, default line-budget/ordinary-budget, windows and reshape regression groups passed; core379 passed/2 ignored, allocator/fmt/workspace all-target Clippy and docs with warnings denied passed. PR latest reviewed-head CI and one fresh Astra final review gate integration.


Measured source `a044cdff72bce5dc546a119a1264592827c32c87`, custom source `ba0920158d302f96d4f21dde0168ada931f020cac23007814badf4ea80e39cdb`, core `0ed16ba57ea875f1dbdcff2d8b2c88ac3e65d8f052f6f7713a1d8d4a3c121aaf`, standard harness `7c139740f9da6bc5ab72420426c81ad5456569e36a99b18b2266fe392b18f80b`. Raw `shared-ligature-edges-raw.json.gz` SHA-256 `dcade54eeb2f574588a702b7237df7aa3333605de77b66f2b2f0b6be4e255eb0`, 9050415bytes. Manifest: [shared-ligature-edges.json](data/shared-ligature-edges.json).
