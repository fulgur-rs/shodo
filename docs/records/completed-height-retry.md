# Completed geometry on height retries

shodo-c91.4 retains one completed, owned Line after a clean height rejection. An identical retry checks its saved needed height; acceptance moves the Line out. No public API changes and no Line clone. Ordinary accepted lines are never retained.

## Eligibility and ownership

The key includes paragraph identity, full token, sanitized width and offsets, all line options, float cursor, built/live font generations and atomic generation/revision. Final sanitized height is excluded. Height sanitization and the per-line edge budget reset still execute on hits. Different owners/tokens, done/block/invalid results, builder/intrinsic calls and any shrink invalidate the entry.

Planned/annotation/alternate first-line paths, max_graphemes, warnings or suppressed warnings, saturation and selected warned Ruby owners use the original path. Alternate first-line bypass affects that line, not later normal continuations. A child-warned paragraph may contain eligible clean suffix lines. Root warning emptiness alone does not establish clean child geometry.

Retention additionally requires the existing same-owner/token PartialLine, avoiding extension of root paragraph/font lifetime beyond the existing raw-scan cache. Tabs and other raw-cache bypasses remain unoptimized. Newly held shared run metadata is distinct from owned geometry; 64 KiB is not a whole-paragraph or RSS bound.

The single-entry bound includes its header/key, context pointer, unused Vec capacity, optional stores, overlays, pending fragments and recursive Ruby heap without double counting embedded headers. Checked arithmetic and a recursion limit fail closed. Shared Arc payloads are separate.

| Independent rejection/owner probe | Completed | Header + owned heap B | Root alive after public drop before/after |
|---|---:|---:|---|
| plain, repeats 1, width 80, graphemes None | True | 528 + 292 | True/True |
| plain, repeats 2048, width 80, graphemes 3 | False | 0 + 0 | False/False |
| ruby, repeats 32, width 80, graphemes None | True | 528 + 3292 | True/True |
| plain, repeats 1024, width 100000, graphemes None | False | 0 + 0 | True/True |

The independent allocator frees exactly header + owned heap while separately held shared owners stay alive; context release leaves every weak root dead. The extra context pointer is eight bytes on this target. These controls show no new root lifetime extension. Oversized geometry is not retained.

## Output and actual work

Six pinned before/after builds (counter-free, allocation-only, actual-work observer) cover237 rows. Standard54 runs both AllLines and PageRetry. Rich inputs include first-line, Ruby, forced break, block, float, widths80/240, direct/one/four rejection, bounded acceptance, an actually disabled FontCollection cache and resource/child-warning controls. Every accepted recursive Line and ordered warning matches; rich cases additionally serialize all LineResult events, needed-height bits and float cursor/position. Standard snapshots preserve their existing float counts, not complete float events. Source/glyph/font/paint/cluster/continuation geometry is exact, normalizing ephemeral owner identity while retaining registered face index. Fixed fonts and all licenses are archived; system discovery is disabled.

Actual root/annotation constructors, metrics, Ruby placement, raw scan, both shaper call sites and Line clones are observed at their production operations. Observer allocation windows exactly equal allocation-only windows. Existing shaper reuse is unchanged. Clean four-rejection lines construct once instead of five times; selected clean Ruby children follow the same reduction. Line clones remain zero. Warned child constructors are unchanged; their control saves only clean suffix work.

## Requested allocation and counter-free time

Core422 passed, two ignored; allocator/probe, fmt, all-target Clippy and docs pass. Strict default54 matches all seven operation digests against the byte-exact previously verified main baseline. Checks, six-build verification and independent ownership probes finish before final timing.

Counter-free release binaries run on CPU10 in four fresh ABBA processes with forward/reverse case order, seven samples and two warmups. Eight preselected cases have four additional processes each, filtering unrelated paragraph construction. Each operation starts a fresh context. Paragraph/font construction, oracle snapshots and validation are outside cost. Rich cases drain warnings and dispose rejected results inside cost; standard cases drain warnings after cost. Output release and context release have separate allocator windows.

| Case | Root constructions before→after | Annotation constructions before→after | Calls | Gross B | Net B | Peak-extra B | Before/after ms | Ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| rich/plain/32/80/default/height/0 | 96→96 | 0→0 | 4735→4735 | 531756→531756 | 135040→135040 | 136495→136495 | 0.4532/0.5416 | 1.1953 |
| rich/plain/32/80/default/height/4 | 480→96 | 0→0 | 11263→4831 | 1223852→582444 | 135040→135040 | 136495→136495 | 0.8440/0.5233 | 0.6200 |
| rich/ruby/32/80/default/height/0 | 40→40 | 48→48 | 32007→32007 | 3765312→3765312 | 1261917→1261917 | 1264181→1264181 | 2.9546/2.9305 | 0.9918 |
| rich/ruby/32/80/default/height/4 | 200→40 | 240→48 | 57351→32047 | 6280416→3786432 | 1261917→1261917 | 1264181→1264181 | 4.6690/2.9482 | 0.6314 |
| rich/first-line-ruby/32/80/default/height/4 | 205→45 | 240→52 | 57928→33084 | 7413910→4968222 | 1261941→1261941 | 1264193→1264193 | 4.6081/2.9944 | 0.6498 |
| rich/float/32/80/default/height/4 | 480→96 | 0→0 | 11936→5376 | 1262956→610284 | 135684→135684 | 136619→136619 | 1.1173/0.6236 | 0.5581 |
| rich/plain/32/80/height-accept/height/0 | 96→96 | 0→0 | 4735→4735 | 531756→531756 | 135040→135040 | 136495→136495 | 0.4533/0.8475 | 1.8697 |
| standard/latin-long/64/PageRetry | 2304→1152 | 0→0 | 278875→239709 | 31367406→27310342 | 1862306→1862306 | 1897271→1897271 | 31.1681/29.7301 | 0.9539 |

| Fresh selected case | Before/after ms | Ratio |
|---|---:|---:|
| rich/plain/32/80/default/height/0 | 0.4682/0.4649 | 0.9930 |
| rich/plain/32/80/default/height/4 | 0.8669/0.5014 | 0.5784 |
| rich/ruby/32/80/default/height/0 | 2.7400/3.1091 | 1.1347 |
| rich/ruby/32/80/default/height/4 | 5.8105/4.1591 | 0.7158 |
| rich/first-line-ruby/32/80/default/height/4 | 4.6606/4.1345 | 0.8871 |
| rich/float/32/80/default/height/4 | 1.3919/0.5804 | 0.4170 |
| rich/plain/32/80/height-accept/height/0 | 0.6810/0.4661 | 0.6844 |
| standard/latin-long/64/PageRetry | 31.9618/27.8655 | 0.8718 |

Largest whole-set time ratios: rich/plain/32/80/height-accept/height/0 1.8697, rich/ruby-child-run-one/1/240/default/height/0 1.7854, rich/ruby-child-run-one/1/80/default/height/4 1.6258, standard/latin-short/1/AllLines 1.6161, standard/preserved-tabs/64/AllLines 1.5873, rich/plain/1/80/edge-zero-warning-zero/height/0 1.5467, standard/mixed-scripts/64/AllLines 1.5297, standard/preserved-tabs/8/AllLines 1.4966. All237 rows and original samples remain; selected and sequential results are distinct.
gross increases: rich/block/1/240/default/height/1, rich/block/32/240/default/height/1, rich/first-line/1/80/default/height/1, rich/forced/1/240/default/height/1, rich/forced/32/240/default/height/1, rich/plain/1/240/default/height/1, rich/plain/1/240/shape-cache-zero/height/1, rich/plain/1/240/warning-zero/height/1, standard/preserved-tabs/64/PageRetry.
net increases: none.
peak-extra increases: rich/block/1/240/default/height/1, rich/block/1/240/default/height/4, rich/forced/1/240/default/height/1, rich/forced/1/240/default/height/4, standard/japanese-long/1/PageRetry, standard/japanese-long/64/PageRetry, standard/japanese-long/8/PageRetry, standard/japanese-short/1/PageRetry, standard/japanese-short/64/PageRetry, standard/latin-short/1/PageRetry, standard/many-short-latin/1/PageRetry, standard/many-short-latin/64/PageRetry, standard/many-short-latin/8/PageRetry, standard/nested-atomic/1/PageRetry.

Requested calls/gross/freed/net and peak-extra are separate. Fewer repeated allocations do not establish lower retained bytes or process RSS. Entry allocation/checking adds first-rejection work; direct controls and adverse timings are retained. Whole-operation peaks can remain unchanged. Shared-host and process/order variation limit interpretation of individual wall-time ratios.

Decision: adopt the bounded clean-retry path to remove repeated actual geometry construction and transient allocation, with the measured time/memory tradeoffs above. No general frame-budget, cold-start or RSS improvement is claimed. No switch blocker or cap increase.

## Controls, provenance and reproduction

Two earlier proposals are preserved with exact source/producers/reports, excluded from adopted timing. The first named a Paragraph-only zero-cache limit while its FontCollection remained enabled. The second retained grapheme-limited geometry without an existing raw owner: a1,236-byte entry prolonged about15.6 MB of shared paragraph payload. It also admitted real child-only build warnings invisible to the root warning sink. Independent RED tests motivated the final owner and child guards.

Constructor, intervening intrinsic/build, owner-lifetime and real child-warning RED logs are retained beside GREEN. A verifier-only assertion originally excluded an entire child-warned paragraph. Its correction distinguishes rebuilt warned children from eligible clean suffixes; both versions, failing log and hashes are archived. Runtime/producers/captures were unchanged by this correction.

Original 7d771b4ed5c954150300f62a314a4f14098879ab → measured candidate d6994b24c873f2ebade9826927c40e5628466002. Engine hashes 2980ab083449c4ab8c0fcae9f6565248b7d9a959d8a0b4f700e84d166cd9ee6f → dce9c8a8e721b0a4f0d1028793c6b7eb8020e0fa5c3a1a4d32d42232be8bdec4; harness 7c139740f9da6bc5ab72420426c81ad5456569e36a99b18b2266fe392b18f80b. Later documentation commits do not substitute measured engine bytes.

Raw completed-height-retry-raw.json.gz: 17170784 bytes; SHA256 716a8d8ac0399bcceca029b438cde38440c96d1bca6a9016495ee0306c0c7178. Codec restores 54 original report streams totaling 5345481802 bytes exactly, without numeric reserialization. Includes source/fixtures/fonts/licenses, binary profiles/identities, both rejected proposals, frozen producers, verifier rulings, all checks and default54 reference proof.

Reproduce: cargo run --release -p shodo-bench --example completed_height_retry; allocation-counting separately. SHODO_COMPLETED_SAMPLES, SHODO_COMPLETED_REVERSE and exact SHODO_COMPLETED_CASE control samples/order/pre-build filtering. Archived capture, verify-final, ownership_probe, standard54, timing, package and final_proof recipes require explicit absolute-path adjustment. One fresh Astra whole-branch review and latest-head CI gate integration.
