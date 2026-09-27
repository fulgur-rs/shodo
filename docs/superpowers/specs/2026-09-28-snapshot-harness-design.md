# Fixed real-glyph snapshot harness design

Issue shodo-p2m.7. Build from 7225194da8207a8c6bf23d395defe0e05066eb25 in the owned `target/worktrees/shodo-snapshot`, branch `feat/snapshot-harness`. This architectural design serves the existing autonomous issue workflow; it is not represented as human-reviewed. Native inline execution, one whole-branch independent review and one material-fix pass. Issue .6 is excluded and the S4 spike remains unmerged.

## Purpose and scope

Generate PNGs from accepted public shodo output, detect differences against committed expected images, and make expected/actual/difference images reviewable together. Fixed real fonts, glyph IDs, retained FontData, normalized variation coordinates, sizes, positions and baselines drive outline painting. Never shape the source text again. Numerical geometry accompanies images so that whitespace/source changes invisible in a bitmap remain detectable. Existing browser comparison remains an independent numerical check.

Use the existing fixture crate and its `examples/support/glyph_paint.rs` painter, with focused snapshot support modules and a registered example command. A new dev crate would duplicate the fixture/painter boundary; Python orchestration would add an image decoder and weaken typed layout integration. No new rendering dependency enters the main library. Retain Rust 1.89 and the existing CPU Skrifa 0.44.0/Tiny-Skia 0.12.0 renderer. Pin those fixture renderer dependencies to these actually resolved versions for reproducibility.

## Inputs and rendering

The exact initial matrix is 21 cases: the existing 12 corpus IDs unchanged, plus `shared-ffi-color`, `arabic-wrap`, `nested-atomic-baseline`, `preserved-tabs`, `normal-white-space`, `hanging-white-space`, `indent-baseline`, `japanese-kinsoku`, and `float-pages`. All use the shared three OFL fixture faces with system discovery disabled. Scale is 1, canvas is 512 by 1024 pixels, white opaque background and a 10-pixel origin margin. The existing painter's glyph outlines are unhinted and antialiased; its returned pixels are copied into this fixed canvas without rescaling. Output exceeding the fixed canvas fails explicitly rather than clipping silently.

Corpus cases use their existing width/font-size/language/direction. Structural cases use Latin 20px, line-height 24px, width 180px unless their settings below override it. Every effective setting and font SHA256 belongs to the expected manifest.

- Shared ffi: three source nodes containing `f`, `f`, `i`, font size 32, width 200. Paint the single accepted glyph once, using its first source owner's red color; show the middle source node's link/underline rectangle separately in blue using offset mapping and selection geometry.
- Arabic wrap: three inline nodes `سل`, `ام `, `سلام`, Arabic font 32px, line-height 48px, RTL, width 65px. Capture actual connected glyphs and accepted line boundaries.
- Nested atomic: four nested inline boxes with end padding 2px, a forced first-line break, a 20 by 20 atomic with baseline 16px and zero margins; green caller-painted atomic and real surrounding glyphs.
- Preserved tabs: `One  two\tthree\nFour five.`, pre-wrap, Latin16px, width90px, tab stop16px.
- Normal whitespace: `One   two\n  three   four.`, collapse/wrap, width90px.
- Hanging whitespace: `One two   three   `, pre-wrap, width90px; retain trailing source/geometry even when trailing whitespace paints no ink.
- Indent/baseline: `Alpha beta gamma delta.`, first-line indent10px, line-height24px; show baseline guides as a separate overlay, never as a substitute for glyph painting.
- Japanese kinsoku: `「日本語」、句読点。読みやすい文章。`, fixed CJK16px, language ja, width90px; record actual line boundaries without claiming unsupported typography is complete. This uses only characters present in the pinned CJK subset.
- Float pages: fixed Latin16px/line-height20px, right float20 by50px, width80px then100px after the first accepted line and a fragment move consuming20px. Text `aa bb cc dd ee ff gg hh ii jj`. Use the shared caller float driver and unchanged source/token protocol; show each page in its own panel, retaining the remaining float height and right-edge placement. Also exercise one height rejection and unchanged-token retry. Never concatenate pages at overlapping origins.

Geometry records accepted line source ranges, block/inline sizes, baselines, glyph IDs/clusters/positions/font fixture identity/variation/synthesis, atomic rectangles, source annotations and page/float placements. Process-local ParagraphId/FontId values are excluded. Painter glyph count must equal accepted glyph count and each render must contain actual glyph ink. Bound line/float retries and fail on nonprogress or unsupported output.

Initial renderer scope is horizontal monochrome TTF/CFF outlines and caller-painted atomic/float rectangles. Synthetic weight/skew, missing outlines, color/bitmap glyphs, arbitrary webfonts, vertical layout and general CSS decoration/DOM are explicit unsupported errors or documented noncoverage. Do not bless a synthetic fallback or blank screenshot as an expected result. Existing painter checks for synthesis remain intact.

## Comparison and publication

Compare decoded premultiplied RGBA bytes, not compressed PNG bytes. Tolerance is zero: any changed pixel, dimensions, geometry, case settings or font/renderer fingerprint fails. Report changed pixel count and dimension/metadata differences. Difference images highlight changed or unmatched pixels in opaque magenta on white; expected and actual are copied into the report directory. Missing/unreadable expected images are failures, with actual image, diagnostic and HTML entry retained. Successful comparisons also produce a report.

Committed expectations live at `dev/fixtures/snapshots`, with `manifest.json`, `<id>.png` and `<id>.geometry.json`. Initial expectations are generated explicitly, visually reviewed by the implementation/reviewer and committed through the authorized PR workflow; do not claim separate human approval. Expected images define regression behavior, not browser/WPT conformance.

The command is `cargo run -p shodo-fixtures --example snapshots -- --output <new-directory> [--expected <directory>] [--case <id>] [--update]`. Default expected directory is resolved relative to the fixture package, independent of caller CWD. Default mode only reads expectations; missing expectations never create them. Output must be new and disjoint from expected paths, including symlink/canonical ancestor aliases. Reject unknown case, unsafe paths and invalid flags before mutation. A selected case is explicitly reported as a partial check. `--update` requires the complete matrix; reject its combination with `--case`.

Render and validate every case before publishing an update. Stage a full new expected directory, then replace the old directory with a backup/rollback sequence; a failure leaves old expectations or a recoverable backup. Refuse to replace an expected directory containing unknown files, rather than deleting unrelated content. Normal checks must never write expectations, including on missing/corrupt PNG or mismatch. Reports are staged and published even when comparisons fail; render errors get diagnostic report entries and never publish expectations. Existing output directories are preserved rather than overwritten. Concurrent updates are outside the initial command contract; run updates exclusively.

HTML uses relative links to expected/actual/difference PNGs, escaped labels/diagnostics, changed-pixel counts and explicit pass/fail/unsupported/missing status. No executable content is derived from case names or paths. CLI returns nonzero for any failure; successful explicit update returns zero with an update report.

## Tests and CI

Meaningful tests render real fonts and inspect literal ffi ownership/count, atomic size/baseline/second-line placement, preserved-tab source boundaries, indent10px, multiple Arabic lines, Japanese boundaries, float remaining height30px/right edge/fragment/source continuation and height-rejection token stability. Separate fresh font collections must produce identical pixels and stable geometry. Unsupported synthesis must abort without creating expectations.

Comparator tests use hand-built one-pixel RGBA fixtures: identity passes, one changed pixel fails with count1 and a magenta difference pixel, unequal dimensions fail, missing/corrupt PNG fails, and changed font/settings/geometry fails. CLI tests run the real command against copied expectations: mutate/delete one expected image, assert failure plus PNG/HTML artifacts and byte-identical surviving expectations; normal missing baseline never writes it; explicit full update is the only creation path; invalid selection/overlapping/symlink paths preserve existing files; a late rendering failure cannot partially update expectations.

Register the example for workspace testing. Its check against committed expectations must execute in ordinary tests. CI additionally invokes the report command and uploads the report with `if: always()` so failed comparisons are inspectable. No CI command supplies `--update`. Document check, inspection, intentional update/review, pixel semantics, unsupported scope and baseline provenance. Verify stable/MSRV workspace, Clippy, docs, fmt/diff, Python/generator, strict browser comparison, no-default and wasm; one final reviewer and exact HEAD CI before merge/close/owned cleanup.

Reference: [Parley tests](https://github.com/linebender/parley/tree/main/parley_tests), whose README separates current and accepted images and uses explicit acceptance/report commands. The shodo harness shares the existing shodo painter rather than copying Parley code.
