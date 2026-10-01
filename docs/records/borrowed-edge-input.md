# Borrowed edge shaping input

shodo-c91.3 feeds unedited edge windows to the existing shaping core through private borrowed scalar/metadata views and stack UTF8 context. The same available up-to-five context characters fit in20 bytes. A conservative compatibility predicate keeps adjacent merge and replacement/font-substitution inputs on the original owned path. Public APIs, limits, output ownership and cache policy stay unchanged. The single-item fast path avoids repeated partition searches only when a second item cannot enter the original stop boundary.

## Contracts and measured work

113 inputs × cold/warm =226 complete Line snapshots and ordered-warning rows from public/manual/internal drivers across six pinned builds; two actual glyph-build refusals0/1 preserve actual25. Non-Line events drive iteration but this bench does not independently serialize the full LineResult event sequence; existing core tests cover event-return/progress contracts. Existing54 standard cases, first-line/Ruby/forced/block/float/offset/budgets/warnings/saturation/cache controls remain. New actual soft-hyphen edits, missing-font stub, giant combining input/run-eight and explicit axes are included. Scalar offsets/end/item/grapheme flags, font/script/bidi/lang/features/orientation, glyph/run/cluster mapping, recursive Ruby, geometry, paint, continuation and ordered warnings match. Actual positive edited copies remain.

The axes fixture derives an fvar table from the existing real Latin font, with normalized coordinates[1,-1]. It exercises metadata and coordinate propagation; its outlines are unchanged, so it is not a variable-outline performance claim. Original snapshot helper is unchanged; the new helper admits only exact pinned stub/derived bytes and retains original matched-font glyph guards. System font discovery is disabled.

Test-only actual Scalar Clone observation fails26 versus0 on the original, then passes0 for real/missing fonts and budgets1/1024. Literal mixed UTF8 contexts, empty/compatible item boundaries, owned merge full output and edited substitution copies are independently tested. Disposable trace builds observe real Clone and seven real cache events. Whole shape_window_edit allocation spans include shaping/output work and are not labeled input-only bytes. All four operation allocation scopes are exactly neutral against allocator-only builds, and before/after cache/shaper events are identical.

## Requested allocation and counter-free time

Required tests, Clippy/fmt/docs, strict standard54 and independent six-build verifier finish before final timing. Four CPU10 fresh processes use ABBA source order and forward/reverse input/history order, seven samples and two warmups. Each layout starts a new context; warm history performs a complete prior layout in a separately measured preparation scope. Selected-case fresh processes additionally isolate earlier input allocator history. Font/paragraph construction, snapshots, warning drain and validation remain outside operation windows. Counter-free release binaries alone supply adopted time; formal/prototype ns are excluded.

| Input/history | Edge Scalar copies before→after | Gross B | Net B | Peak-extra B | Before/after ms | Ratio |
|---|---:|---:|---:|---:|---:|---:|
| standard/latin-long/64/cold | 23066→0 | 28909186→27144454 | 1518242→1518242 | 2304674→2304674 | 42.5003/29.7741 | 0.7006 |
| standard/latin-long/64/warm | 23072→0 | 28863570→27098234 | 1320022→1320022 | 2106454→2106454 | 66.7128/34.3349 | 0.5147 |
| standard/arabic-long/64/cold | 177487→0 | 82768688→76183288 | 2530024→2530024 | 2923240→2923240 | 107.0534/401.9176 | 3.7544 |
| standard/arabic-long/64/warm | 177531→0 | 82723390→76136610 | 2192550→2192550 | 2585766→2585766 | 151.2957/229.6602 | 1.5180 |
| rich/edge/32/80/default/width/cold | 1998→0 | 2048808→1724340 | 192769→192769 | 241921→241921 | 2.8248/3.2556 | 1.1525 |
| rich/edge/32/80/default/width/warm | 2009→0 | 2005249→1679593 | 20116→20116 | 105291→105139 | 2.8046/6.0624 | 2.1616 |
| rich/hyphen/8/8/default/width/cold | 615→144 | 847776→757608 | 127695→127695 | 152271→152271 | 0.8884/2.3244 | 2.6165 |
| rich/hyphen/8/8/default/width/warm | 256→144 | 642064→612748 | 56928→56928 | 81504→81504 | 0.5438/0.7988 | 1.4689 |
| rich/giant/1/8/default/width/cold | 3→0 | 17867→16815 | 5559→5559 | 7443→7443 | 0.0219/0.0203 | 0.9251 |
| rich/giant/1/8/default/width/warm | 0→0 | 9763→9763 | 2520→2520 | 4206→4206 | 0.0066/0.0084 | 1.2699 |
| rich/variable/8/8/default/width/cold | 712→0 | 1882900→1675084 | 338672→338672 | 436976→436976 | 1.8389/3.0060 | 1.6346 |
| rich/variable/8/8/default/width/warm | 168→0 | 1431008→1357952 | 166336→166336 | 264640→264640 | 1.2975/3.2237 | 2.4845 |

| Fresh selected input/history | Before/after ms | Ratio |
|---|---:|---:|
| standard/latin-long/64/cold | 27.9391/26.6017 | 0.9521 |
| standard/latin-long/64/warm | 38.9456/28.0112 | 0.7192 |
| standard/arabic-long/64/cold | 74.6330/80.9945 | 1.0852 |
| standard/arabic-long/64/warm | 85.9981/75.7335 | 0.8806 |
| rich/giant/1/8/default/width/cold | 0.0271/0.0251 | 0.9266 |
| rich/giant/1/8/default/width/warm | 0.0102/0.0097 | 0.9523 |
| rich/variable/8/8/default/width/cold | 1.4014/1.3372 | 0.9542 |
| rich/variable/8/8/default/width/warm | 0.7575/0.7379 | 0.9741 |
| rich/hyphen/8/8/default/width/cold | 0.6893/0.6628 | 0.9616 |
| rich/hyphen/8/8/default/width/warm | 0.4212/0.4163 | 0.9884 |
| rich/ruby/32/80/default/width/cold | 7.6419/4.0805 | 0.5340 |
| rich/ruby/32/80/default/width/warm | 4.9089/3.5851 | 0.7303 |
| rich/edge/32/80/default/width/cold | 6.6233/9.7933 | 1.4786 |
| rich/edge/32/80/default/width/warm | 4.2621/5.8648 | 1.3761 |

Additional cache-edge diagnostic uses31 samples per history in four ABBA fresh processes. All full outputs match; these controls remain separate from the adopted seven-sample matrix. Whole-process CPU clocks include preparation/oracles/report and are not layout-only CPU.
before: layout medians {'rich/edge/32/80/default/width/cold': 3739408, 'rich/edge/32/80/default/width/warm': 4129769}; whole-process user 0.637985s/system 0.025520s, minor/major faults 7145/1, involuntary switches 382.
after: layout medians {'rich/edge/32/80/default/width/cold': 3696034, 'rich/edge/32/80/default/width/warm': 3994201}; whole-process user 0.607138s/system 0.027732s, minor/major faults 7145/0, involuntary switches 259.
after: layout medians {'rich/edge/32/80/default/width/cold': 3863941, 'rich/edge/32/80/default/width/warm': 2582712}; whole-process user 0.514497s/system 0.032031s, minor/major faults 7144/0, involuntary switches 383.
before: layout medians {'rich/edge/32/80/default/width/cold': 3400173, 'rich/edge/32/80/default/width/warm': 2048681}; whole-process user 0.445283s/system 0.025900s, minor/major faults 7146/0, involuntary switches 154.

Observed sequential time ratios (high process-to-process variation): rich/plain/32/240/default/width/cold 6.9331, rich/first-line-ruby/32/80/default/width/warm 4.6415, standard/arabic-short/8/warm 4.5202, standard/arabic-long/8/warm 4.5202, rich/ruby/32/80/default/width/warm 4.3916, rich/first-line-ruby/32/240/default/width/warm 4.0367, standard/arabic-short/8/cold 3.9982, rich/float/32/240/default/width/cold 3.8852. All226 rows remain in the manifest, including adverse cases. Gross or peak-extra increases: none.

Calls/gross/freed/net/whole-operation peak-extra are separate, with preparation/output-release/context-release also retained. Removed transient input copies do not imply a retained-byte reduction. Scope peak-extra uses its own live baseline and is not process RSS. Warning-drain frees happen outside windows; four-scope residuals are not leak evidence. Shared paragraph/font ownership is unchanged. Selected-input and sequential timings expose allocator-history variation; one ratio is not a universal guarantee.

The shared host has recorded CPU/memory/I/O pressure and substantial between-process variation even within the baseline. These observations do not establish the cause of any individual ratio. Neither sequential nor fresh time establishes a universal CPU improvement.

Decision: adopt private borrowing to remove redundant unedited scalar/context allocations while retaining required edits/merges. CPU evidence and adverse conditions are reported above without a frame-budget/RSS/parallel/cold-start guarantee. No switch blocker, cap increase or new dependency.

## Verification and provenance

Core398passed2ignored, allocator/probe, fmt, all-target Clippy and docs with warnings denied pass. Current0.0.5 standard54 exactly matches all conditions and seven-operation digests against the untouched .2 measured source, whose engine bytes equal this issue baseline. Reference reuse is separately pinned; no original report metadata is rewritten.

Preserved controls include the original26-copy RED, a rewrite matcher that rejected before updating shape.rs and therefore remained RED, two test-style Clippy rejections and initial new-font identity rejection by the unchanged strict helper. None is adopted as timing or hidden by relaxing output/budget contracts. All formal producer recipes remain frozen across six builds.

Original `8ca0f5ff231a6a613bef71ceca2ab6d9b3ec43cc` → measured candidate `859fdc560c88cf609001c96174dcf79f66da690f`. Engine hashes `10bb87ad6c2d5abb11b7ace6564b31310f3b85e5f99830c370940816a7ce505f` → `aa235572e8256e34f4272bce6079fe697e4e8610d13dde65ea23f8e310b1d26e`; harness `7c139740f9da6bc5ab72420426c81ad5456569e36a99b18b2266fe392b18f80b`. Documentation commits after measurement do not substitute runtime bytes.

Raw `borrowed-edge-input-raw.json.gz`: 8032660 bytes, SHA256 `19892866c6fa846efe6354dd142e9e3e01b878f5f54f340b7b235f16e30f7355`. Codec restores 42 original UTF8 report streams totaling 2697700649 bytes using exact output strings/text pieces, preserving original SHA/length without numeric reserialization. Includes complete source/harness/fixed fonts/licenses/derived font, six binary profiles/SHAs/observers, recipes/pins/logs and standard54 provenance.

Reproduce: `cargo run --release -p shodo-bench --example edge_input_borrow`; allocation-counting separately. SHODO_INPUT_SAMPLES controls positive samples, SHODO_INPUT_REVERSE reverses order, SHODO_INPUT_CASE selects an exact query key before unrelated paragraph builds. Archived capture/verify/balanced/fresh/package/final_proof recipes require explicit absolute-path adjustment. One fresh Astra whole-branch review and exact latest-head CI gate integration.
