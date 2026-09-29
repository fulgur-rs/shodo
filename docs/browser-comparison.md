# Fixed-font Chrome line breaking comparison

This harness records real browser measurements and checks public `next_line`
against the saved widths. Ordinary Rust tests need no browser and never replace
expectations. This detects numeric changes; green tests do **not** mean every
Chrome break matches shodo or that WPT has passed.

## Inputs and current evidence

`dev/fixtures/assets/browser-inputs.json` contains 55 materialized cases: 16 seeds
for each of Latin, Japanese and Arabic, plus nested inline, differently colored
shared `ffi`, Arabic wrapping, pre-wrap/tab, Japanese punctuation, an empty
inline-block and a supplementary-scalar case. The versioned xorshift32 generator
samples the existing original fixture corpus; no external corpus or font is
added. The input file is public through `shodo_fixtures::browser::cases()` and its
matching `build()` helper for later snapshot/benchmark consumers.

Use the existing Latin/CJK/Arabic subset bytes and families, face index 0, original
font/corpus provenance and licenses. Both engines load these bytes; shodo disables
system discovery. Sizes are 16/16.25/16.5/16.75px, line-height 40px, tab-size 8,
line-break normal, no hyphenation, no transforms, no synthesized style. Arabic
uses RTL/lang=ar, Japanese lang=ja. Nested spans inherit shaping styles; color
changes do not deliberately split shaping. Atomics have a 24×18px empty border
box, no margins, and baseline at the bottom edge.

The checked-in browser is **Chromium 152.0.7977.82 Arch Linux**, collected on Linux
x86_64. Two independent disposable-profile runs reproduced the whole data JSON,
including metadata and geometry. There are 458 measured width probes. At the
original widths, 408 source endpoints match; 50 differences remain. There are 50
measurable target transitions: five initial targets are already present at the
1/64px minimum probe, and explicitly have `boundary_subpixels: null` (overflow/
first unbreakable segment), rather than an invented zero-width transition.

## Offline checks and reports

From the repository root:

```sh
cargo test -p shodo-harness --test browser_inputs --test browser_comparison
cargo run -p shodo-harness --example browser_compare -- --check
cargo run -p shodo-harness --example browser_compare -- --json
cargo run -p shodo-harness --example browser_compare -- --transitions
cargo run -p shodo-harness --example browser_compare -- --atomics
python3 -m unittest discover -s dev/fixtures/tools -v
```

The plain/JSON report prints **all** raw differences, even known ones, with case
ID, seed, full original input, font ID, size, unadjusted width, Chrome endpoint and
shodo endpoint. `--check` additionally enforces the reviewed exact difference
ledger, all transition thresholds and atomic geometry pairs. Unknown differences,
changed expected/actual values, duplicates, and stale exceptions after an
improvement all fail. A changed dataset/recorder/corpus/font hash also fails.
Normal workspace CI runs these saved-data Rust checks and Python tool tests;
there is no Chrome installation or automatic expectation update in CI.

## Collect and update deliberately

Python standard library and a Chrome/Chromium executable are sufficient. There
is no Playwright, WebDriver, Wasm build, Rust browser toolchain or FontTools
requirement for browser collection (FontTools is still needed for font updates).

```sh
python3 dev/fixtures/tools/browser_cases.py --check
python3 dev/fixtures/tools/collect_browser.py --browser /usr/bin/chromium \
  --output /tmp/shodo-chromium-new.json
python3 dev/fixtures/tools/collect_browser.py --browser /usr/bin/chromium \
  --output /tmp/shodo-chromium-repeat.json
```

Compare both result files, their source endpoints/thresholds and metadata. The
collector starts a loopback-only server, hashes and loads the fixture fonts with
FontFace, and uses headless `--dump-dom` with a 30-second virtual-time budget. It
gets a fresh temporary profile, never an existing browser profile. The subprocess
has a 90-second wall timeout (`--timeout` overrides it). Font-load errors, wrong
hashes, a browser failure/timeout, absent result or incomplete/invalid records
fail before atomic replacement of the requested output. Restricted environments
may need permission for Chromium/Crashpad sockets and the loopback listener.

For an intended input change, edit/version `tools/browser_cases.py` and run it
without `--check` to materialize the new inputs; the collector and Python tests
reject stale materialized cases. Font/corpus changes follow the existing fixture
update process first. Do not edit font bytes to force browser equality.

After reviewing the new capture, run the collector with the checked-in path:

```sh
python3 dev/fixtures/tools/collect_browser.py --browser /usr/bin/chromium \
  --output dev/fixtures/assets/browser/chromium.json
```

Then run all reports. Review every changed break and transition. Update
`assets/browser/differences.json` **manually**, retaining only justified exact
observations, their category/evidence/issue linkage, input hash and SHA-256 of the
actual capture file. Also review the 50 transition records and 9 atomic geometry
pairs stored there. There is intentionally no command that automatically accepts
current shodo mismatches or adds blanket tolerance. Run offline tests, Python
tests, full workspace/MSRV and [development checks](../CONTRIBUTING.md#checking-a-change) before committing the intentional
capture/ledger diff. Human review of a new capture remains meaningful even if a
new browser version is expected to differ.

## Original source positions

The browser scans Unicode **scalars** in logical DOM order, using Range rectangles
for actual text nodes and an element rectangle for the atomic marker. It detects
a later line by y, not by RTL x order, and records the first scalar belonging to
that later line. These fixed cases have one inherited font size and 40px line
height, so adjacent line tops are separated by 40px. The current recorder is not
an arbitrary-DOM line-box extractor: mixed font sizes, arbitrary vertical-align,
transforms, ruby, vertical writing and tall/ multiple atomic boxes need another
measurement contract. The common builder rejects more than one atomic box.

Normal collapsible spaces/tabs have no independent visible line box: they are
assigned to the preceding source range until the next non-whitespace scalar.
Pre-wrap spaces/tabs are measured and segment breaks are skipped as zero-ink
controls; the next visible scalar determines the endpoint. These rules describe
an original source-consumption boundary, not painted glyph count. The fixed
pre-wrap input includes an interior newline; the first-line probes do not treat
newline as a visible glyph.

JavaScript string positions are UTF-16 code units. Each scalar advances by one or
two units; no scalar is split. The recorder also encodes the prefix with
TextEncoder to obtain UTF-8 bytes. Python validation and Rust conversion check
both endpoints; e.g. `A𠮷é` boundaries are UTF-16 `0,1,3,4` and UTF-8 `0,1,5,7`.
UTF-16 offset 2 and UTF-8 offset 2 are invalid interiors. Every saved sample is
checked, including the supplementary case.

shodo endpoints come from the actual Line's processed range and offset mapping,
then the original node-local source range, concatenated across parts. Do not
compare DOM UTF-16 offsets directly with processed UTF-8 positions. Atomics are
one source U+FFFC (one UTF-16 unit, three UTF-8 bytes), represented by generated
NodeId mapping rather than a text-node offset. The nested/shared-ffi cases retain
separate source nodes even when glyphs are shared.

## Measured differences and their interpretation

| Category | Observations | Evidence and follow-up |
| --- | ---: | --- |
| Boundary drift | 47 | Same target endpoints; shodo transition minus Chrome is -1 to +4 integer subpixels. Every difference lies strictly between the two measured transitions; probes a pixel away agree. Exact observations are retained, with no global slack. Japanese07/13 now differ by only -1/+1 subpixel. |
| Conditional line-end trim policy | 3 | Japanese12: captured Chrome target24 threshold8089, shodo7578 subpixels. Shodo follows CSS Text4 normal end trim for Japanese fullwidth stops; the prior shodo8090 threshold decreases by512 subpixels (8px). Independent Chromium152 fixed-font `日本。` at40px gives end3 with both normal and space-all; shodo normal fits all9 UTF-8 bytes. |
| Resolved hanging trailing tab | 0 semantic / 2 numeric | The preserved-whitespace hanging policy fixes pre-wrap-tab: both consume source end9 at90px. The shodo threshold improves6390→4338 versus Chrome4336, so only the two adjacent probes remain as exact boundary drift. The original diagnostic `white-space: break-spaces` gives end5 at90px and target end9 threshold6389 with initial120px. |

The normal-CSS capture is preserved; diagnostic CSS probes did not replace it.
Punctuation implementation changes matches404→408 and mismatches54→50: eight
old differences disappear and four appear. Japanese07/13 thresholds improve
7504→6968 and9256→8736. Seven former half-em spacing differences resolve;
Japanese13's remaining one-subpixel observation is now classified as boundary
drift. Japanese12's former boundary probe at8089 resolves, while three lower
widths now expose the conditional end trim policy difference. This ledger
records exact browser endpoint observations, not WPT PASS counts.

Conditional end trim follows
[CSS Text4 WD2026-08-14 §8.5](https://www.w3.org/TR/2026/WD-css-text-4-20260814/#text-spacing-trim-property).
Independent Chromium152 controls with `日本」`, `日本、`, and `日本。` at16px
and40px all return end3 for normal and space-all; at48px all return end9.
These controls do not explain every contextual spacing choice inside Chromium.
The saved corpus and all raw transition values remain the comparison evidence.

To reproduce those diagnostics, make a temporary copy of the collector and
recorder, point its ROOT at these fixtures, make the fixture tools importable with
`PYTHONPATH="$PWD/dev/fixtures/tools"`, and set `textSpacingTrim:'space-all'` in
`box.style` and, only for pre-wrap-tab, `whiteSpace:'break-spaces'`. For the target9
transition, set that case's initial width to7680 subpixels (120px) before building
the HTML payload; the diagnostic is a different CSS/input condition and must not
be committed as the normal capture. The numbers above came from actual browser
runs, not from assumed Parley tolerances.

The boundary-drift classification is **observational**, not a proof of every
Chromium arithmetic operation or CSS conformance. shodo uses per-advance rounded
1/64px layout units; the reference Parley test discusses different Chrome
arithmetic. Neither its +1/64px offset, residual slack nor font-size quantization
is adopted here. In particular, the measured Arabic drift can reach4/64px; the
dataset does not hide it under a presumed one-unit tolerance.

The empty inline-block bottom-baseline policy agrees, while the raw line/atomic
baseline differs by **0.203125px** (e.g. Chrome top8/bottom26 vs shodo
8.203125/26.203125). At3812 subpixels, the one-unit break-threshold difference
also moves the atomic to a later 40px line. All nine raw geometry pairs are
checked exactly. This records the metric difference; it does not automatically
classify every baseline discrepancy as harmless.

## Scope and references

This is fixed-input first-line/numeric regression evidence. It does not invoke
wptrunner, certify arbitrary browser/DOM equivalence, compare color rasterization,
or model page-spanning float transitions. Rendering and caller float state have
separate [snapshot tests](snapshot-tests.md) and a
[float integration harness](float-integration-harness.md). These checks do not
establish complete production raikiri integration.

- [Parley browser recorder](https://github.com/linebender/parley/blob/main/parley_tests/linebreaking_browser_recorder/src/main.rs) and [comparison](https://github.com/linebender/parley/blob/main/parley_tests/tests/linebreaking_matches_chrome.rs): inspected as references, not vendored.
- [Official Chrome Headless documentation](https://developer.chrome.com/docs/automation-and-testing/headless): dump-dom and virtual-time budget.
- [CSS Text 3 whitespace positioning](https://www.w3.org/TR/css-text-3/#white-space-phase-2), [tab sizing](https://www.w3.org/TR/css-text-3/#tab-size-property) and [boundary shaping](https://www.w3.org/TR/css-text-3/#boundary-shaping).
- [CSS Text 4 punctuation spacing](https://www.w3.org/TR/css-text-4/#text-spacing-trim-property) and [CSS Inline 3](https://www.w3.org/TR/css-inline-3/): reference contracts for the documented semantic/metric investigations.

## Preserved whitespace policy

The hanging-tab fix uses each whitespace unit's existing style. `Preserve` with wrapping excludes trailing spaces/tabs at soft breaks; forced/end lines retain the part that fits before alignment. `Preserve` with nowrap keeps preserved advances. `BreakSpaces` keeps trailing advances and allows breaks after each preserved space/tab, subject to nowrap and mandatory-break precedence. Selection retains the full tab-stop advance even when line fitting excludes it. Nonzero end padding/borders obstruct preserved whitespace hanging; collapsible trailing spaces retain removal/reordering behavior.

Scanning, float retry caches, prescribed plans and intrinsic measurement use the shared policy. Min-content excludes eligible hanging whitespace; max-content includes conditional trailing whitespace and its tracking. The synthetic regressions use10px glyphs/40px tab stops, and the real Latin case pins source end5 at4337 and end9 at4338 subpixels without modifying browser data. Supported first-line font changes preserve the underlying whitespace policy.

CSS Text4 explicitly leaves the hanging behavior of `preserve-spaces` open. Its existing trailing behavior is retained as a compatibility decision, with no new conformance claim. The original Chromium observations, inputs, fonts and recorder are unchanged by this core correction.
