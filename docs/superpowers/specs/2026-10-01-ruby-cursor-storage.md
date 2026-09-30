# Ruby cursor storage (shodo-sbp.6)

First measure real PreparedRuby cut/lane counts, logical and physical cells, requested capacities and retained bytes, cursor change density, whole-build time/calls/gross/net/peak independently. Use fixed real fonts, normal/first-line, columns16/64/256/512, and unchanged default C1024 refusal. Include many short lanes and few long full-span lanes.

If dense cursor storage is a material retained cost and transition compression saves it, implement a shared immutable table with row views keyed by paired-cut ordinal, per-lane sparse transitions and a dense fallback for frequently changing cursors. Preserve every old row unit/class/cursor, duplicate parent-unit rows, normal/first-line source correspondence, spans and legal breaks. Preserve EXACT old count*(1+lane_count) global/per-base Items charges before storage allocation and default refusal; physical compression never relaxes budgets. Keep the correspondence walk/counting algorithm and selected-lane measurement from sbp5. If measurements do not justify complexity, record rejection/defer with evidence.

Keep source/glyph/geometry and warnings/limits, original inputs, first-line/empty/mandatory/shared/nested/RTL behavior. No public API, dependency/font/fixture changes; no raikiri switch gate or saved S4 edits. Separate gross/net/peak/RSS, normal timing from instrumentation and cache/process history. Full required validation, final source-frozen all54 matrix against sbp8, one fresh whole-branch Astra review, PR/all CI/merge/close/owned cleanup.

## Final review refinement

Compare all column capacities plus column headers against a single row-major dense arena, and flatten if larger. For fully active nonempty normal lanes without mandatory parent overrides, each emitted interior cursor must advance: use original walker with direct dense storage to avoid building/transposing dense columns. General partial/mandatory/source-matched paths use final global fallback. Retention includes fixed Arc overhead; requested bytes/peak need not decrease for short dense64 cases. Regression covers raw builder and real normal/first-line8x64 values before retention assertion.
