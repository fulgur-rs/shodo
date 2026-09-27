# Chrome line breaking comparison design

Issue: shodo-p2m.9. Base: b69a5935ae2ecd7ecdbd058fd50867b55fa3235b.

## Intent and acceptance

Collect real Chromium first-line breaks and the smallest 1/64px width preserving a target break under fixed inputs/fonts. Recollection must be runnable, while ordinary Rust/CI comparisons require no browser. Include nested inlines, color boundaries/shared ffi, Arabic line breaking, pre-wrap/tabs, Japanese punctuation and supplementary scalars, and an empty inline-block with a synthesized bottom baseline. Preserve seeds, complete materialized inputs, font hashes, browser version and collection rules. Detect regressions without changing shodo to imitate undocumented Chrome offsets.

This is a new development subsystem. The user has requested autonomous issue implementation, PR/CI/merge and cleanup; that authorization supplies execution/integration choices. It does not approve any still-unseen artifact as human-reviewed. Design and plan receive inline self-review, with one fresh final whole-branch reviewer. S4 remains protected and unmerged; .6 is excluded.

## Approach

Options considered: Rust/Wasm recorder with manual copy (extra toolchain/manual capture); Playwright/CDP client (new runtime/dependency); standard-library Python plus headless Chromium dump-dom (selected: installed browser, no new dependencies). A loopback server serves checked-in fixture bytes and a JS recorder. The browser gets a disposable profile, never the user's profile. FontFace loads and actual font hashes are checked before collection. Timeouts, nonzero exit, absent/incomplete result and script/font errors fail explicitly; output updates require an explicit collector command and atomic replacement only after validation.

Materialized versioned browser cases live with existing fixtures and reference existing corpus/font IDs. A deterministic seed generator builds short Latin/CJK/Arabic cases plus named structural cases. The inputs are available to later snapshots/benchmarks. A development-only Rust module builds matching paragraphs via public APIs, including same-style nested boxes and fixed AtomicSizes. Root dependencies/API remain unchanged.

## Measurement contract

CSS pixel widths are integer subpixels /64. For each case record its first-line source endpoint at its initial width, the minimum width preserving that endpoint (or an explicit no-transition status), adjacent boundary samples, and additional probes away from the boundary. Inspect every Unicode scalar with DOM Range across the original text nodes; do not binary-search code units or assume ASCII or monotonic bidi x. Measure y to identify the first logical line, not visual RTL x order. Empty atomic boxes contribute one U+FFFC in the common source stream, with an element rectangle instead of a text Range. Positions are stored both as UTF-16 and UTF-8; surrogate interiors are invalid. CSS normal trailing/collapsed whitespace needs explicit source-end normalization; pre-wrap preserves text/tabs/newlines. Do not equate raw DOM offsets with processed shodo offsets: offset mapping recovers original node-local source endpoints. Collector measurement limitations must be rejected or documented with an explicit regression case, never silently treated as matching.

Collect an atomic bottom-baseline geometry observation as well as its break behavior. Fixed line-height, no text transforms, no font fallback/system discovery, no letter/word spacing or synthesized font styles. Japanese uses lang=ja and line-break=normal; Arabic uses lang=ar/direction=rtl. DOM wrappers keep inherited shaping styles and differ only in color.

## Comparison and differences

Call public next_line at exact unmodified browser widths. Compare source endpoints and boundary transitions. Report case ID, seed, full input, font/size, width, browser expected and shodo actual. No blanket +1/64px or font-size quantization override. Determine transition differences from measured neighboring widths, distinguish small numeric drift from semantic CSS differences, and retain exact mismatches in a separately reviewed known-difference ledger with cause/evidence and existing issue linkage where appropriate. Unknown mismatches fail; known ones must match exact recorded values so improvement, worsening and stale exceptions are visible. No blanket tolerance, ignored cases or automatically accepting shodo output as browser truth. A diagnostic command exposes the complete comparison report independently of test success.

## Validation and limits

TDD for generation/validation, Unicode mapping, matching paragraph construction and comparison diagnostics. Actual headless collection twice must reproduce measurement data on this browser/font/input combination. Offline workspace stable/MSRV tests, Clippy, docs, formatting, fixture tool tests and exact PR CI. Validate corpus/hash metadata and all priority case families. Browser comparisons are sampled evidence, not WPT verdicts or a full CSS conformance claim. Cross-page floats remain .12/S4; arbitrary DOM/page layout is outside this fixed-input recorder.

References inspected: Parley recorder/comparator (read-only /tmp/shodo-parley-browser-*-reference.rs), CSS Text 3 §§4.1.2/7.3, CSS Inline 3 inline-block baselines, official Chrome Headless documentation. Source implementations are not vendored; special Chrome overrides are not adopted.
