# SHODO-16: paint-only first-line analysis and shaping reuse

The task is to reduce duplicate paragraph construction work when first-line
styles change only paint. The public first-line style and token contracts stay
the same. This branch starts at `c64b8b3`, independently of the held Skrifa PR.

## Scope and choice

Use a guarded reuse path for normal/alternate styles that differ only in
`PaintStyle`. Reuse the processed input, break analysis, bidi analysis and
combine ranges, then reuse selected shape items, glyph storage, shaped runs
and feature arrays. The alternate owns its own style vector and ordinary
paragraph data; existing unit construction, finalization, cursor mapping and
line formatting remain in place.

Retaining those two datasets costs copying and retained storage. It avoids a
new paint-overlay protocol across layout, public views and hit testing. Removing
alternate data entirely is a larger interface change, not required for safely
removing the repeated analysis/itemization/font matching/shaping in this task.

Building only a prefix for size/transform changes is rejected here: build has
no maximum line width and a retained paragraph may later be laid out at any
width, including unbounded width. Shape-affecting first-line styles keep the
complete alternate path. No claim of accelerating that class is permitted.

## Eligibility and effects

- Compare every paired, sanitized style with paint excluded. Non-paint fields
  must match, including the bits of inherited floating-point values and
  explicit variation coordinates. Signed zero must not change exposed run
  variations through reuse. Paint inheritance/resolved-first-line behavior is
  still computed by existing helpers.
- Reuse analysis only without Ruby aggregate/base scopes or external break
  override callbacks, with both warning sinks clean and unsuppressed, and when
  the remaining alternate text/item bounds cover the complete copied output.
  The normal input transformation may be nontrivial if it is identical for
  both style sets.
- Preserve style/text/item accounting and the original alternate limits. An
  insufficient bound uses the old path so the same error kind, limit and
  prefix-based actual count are produced. Do not make the limits cheaper just
  because computational work is skipped.
- Reuse shaping only after analysis qualified, normal shaping emitted no
  warnings or saturation, the remaining shaped-glyph cap covers the complete
  copied store, and both data sets still have bit-identical non-paint styles.
  Otherwise use full alternate itemization/shaping with the reusable analysis.
- No finalization is skipped. Preserve the order of saturation/warning effects,
  source cuts, ligature cursor slices, normal/alternate mappings, decoration,
  first-line paint, floats, blocks, atomics, vertical/composed geometry and
  offset mappings.
- No public API, dependencies, MSRV or runtime switch is added. A test-only
  reference mode exercises the old branch. It must restore its thread-local
  setting after each comparison.

## Proof and validation

First write an analysis-pass regression: paint-only first-line input must use
one `analyze_data` pass, where the baseline uses two. Then prove that real-font
itemization/Harfrust work does not double for the same class. Counters observe
actual work and exist only in tests.

Compare reference/reuse processed text, mappings, breaks, bidi levels, styles,
units, glyphs and public line output at zero/narrow/wide/unbounded widths.
Exercise inherited/resolved paint, decoration, ligatures across source nodes,
capitalization with identical transforms, forced breaks, RTL, CJK, vertical and
combined text. Tests must assert first-line paint changes and later-line paint
does not, in addition to equality against the old path.

Fallback comparisons cover Ruby, shape-affecting styles, callbacks, malformed
paint and numeric outliers with zero/one warning caps, insufficient text/item/
glyph/style budgets, warm contexts and limit errors. Check callback effects,
ordered warnings and saturation messages explicitly; equality must not hide
owners by removing meaningful token fields.

Run the crate suite after each runtime task, then workspace tests, formatting,
normal/accesskit Clippy and required CI before merge. Use one independent
whole-branch review after the implementation and recorded performance decision.

## Measurement and adoption

Measure short and 4,000-word fixed-font paragraphs: no first-line, identical
first-line, color/decoration first-line, and size/transform fallback controls.
Include CJK, RTL, vertical/composed and structural input. Use alternating
baseline/candidate process pairs after both release builds finish. Compare full
public snapshots, intrinsic sizes, ordered warnings, plans and height rejection;
hashes are computed outside timing/allocation windows.

Record raw build timings, separate analysis/shaping stages, allocations,
retained/peak bytes and observed work counts. Normal and fallback controls must
be reported even if they show a regression. Adopt only if measured savings
justify the additional clones and guard cost; otherwise retain the tests and
measurement record without the runtime optimization.

Use `~/tmp` for task scratch and remove it after preserving reproducible inputs,
conditions, raw series, results and rulings in formal records/Plane. Keep the
held SHODO-13 worktree/PR untouched and do not merge it.
