# Ruby candidate ranges (shodo-sbp.5)

Measure only columns and annotation lanes that intersect the candidate's legal paired window. Preserve the surrounding dependencies needed for overhang, nested ruby, and placement.

Preserve source units, glyphs, geometry, legal breaks, limits, global lane IDs, spanning annotations, multiple levels, empty bases, merge, overhang, inter-character RTL, first-line styles and BreakToken continuation. Do not change PreparedRuby storage or paired cuts. Merge's font cap continues to use the original first lane in the level. Do not block switching to raikiri.

Columns are ordered by source units. Prepared lanes are ordered by level and non-overlapping column span. Query both with partition_point; no persistent index. First filter lanes with full-size column arrays, then compact arrays to the selected column window with an explicit original-column offset. Placement and geometry must convert that offset when reading original metadata.

Measure C=16/64/256/512 with fixed fonts and default limits. Separate build from fresh layout, timing from allocation instrumentation, and gross from net/peak bytes. Compare intermediate and final changes and verify output digests. Preserve all standard 54 fixture digests. Record remaining complexity and measured limitations without promising unmeasured speed.
