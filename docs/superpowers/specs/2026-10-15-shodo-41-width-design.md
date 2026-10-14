# SHODO-41: one indexed line-width model

Plane SHODO-41 asks normal line scanning and ruby range queries to share the
paragraph prefix tables, replacing the duplicated `plan::selected_raw` model.
The user authorizes implementation, verification and merge.

`line::range` owns natural selected content, trailing whitespace, spacing,
continuation decorations and source-clipped cluster corrections. Scans carry
the tab origin (container offset plus indent) in a local cursor. Its sparse
cumulative tab and clipped-cluster corrections survive ruby re-entry; scalar
ruby queries retain their own start-specific tab prefix in the paragraph cache. A natural measurement finishes with actual edge-window deltas and the
same punctuation policy for ruby (`None`) or regular lines (`LineOptions`).

Selected lines materialize per-unit widths and edge overlays for painting, but
their content width comes from the indexed measurement. Normal scan, fixed
break plans and retry cache selection use that same selected-line constructor.
Candidate fitting uses indexed natural ranges; overflow extents remain separate
because overflow observes visible unbroken content rather than a selected break.
Keep deferred shaping, grapheme-limited breaks, selected hyphen identity, ruby
adjustments and float retry semantics.

No public API or dependency changes. Preserve glyph positions, breaks, warning
order, cold/warm saturation replay and reshape budget decisions. For extreme
signed advances that overflow and then shrink, use the indexed wide prefix
arithmetic consistently, including tab positions and window delta completion;
the previously divergent sequential selected arithmetic is a test oracle only. Keep the old selected
implementation only as a test oracle, not a production alternative. Existing
range golden records and independent shaping comparisons protect geometry.

Verify tabs at nonzero offsets/indents, atomic revisions, cloned box edges,
trailing whitespace barriers, bidi, discretionary hyphens and clipped ligatures;
verify cold/warm query effects and deferred-candidate/retry work bounds.

The cursor charges unit producers once. Candidate geometry and final selection
read its already-measured sparse prefixes without re-shaping source-clipped
clusters or repeating atomic warnings. Retry restores released paragraph
indexes before reading a retained cursor. Obstruction at each boundary is
indexed once so a long transparent suffix does not multiply candidate work.

Static advances, visual spacing, trailing whitespace and obstruction tables are
immutable paragraph-owned indexes prepared once after finalization. Atomic
widths are zero in the static advances; normal cursors measure only visited
caller atomics and add sparse cumulative deviations. Scalar ruby queries keep
a per-revision atomic prefix so repeated queries do not re-enumerate atomics.
Context/root switches and explicit shrink cannot rebuild the static tables.
Their preparation stores unit saturation for selected-range replay and emits
no earlier build warning.

This adds linear retained width-table memory to each live paragraph instead of
only the context's most recent paragraph. Like existing finalized spacing and
unit tables, it derives from admitted units/glyphs. `max_analysis_bytes` remains
the existing logical analysis-admission contract, not an RSS cap or a quota for
finalized spacing/width tables. Context shrink releases mutable scalar/index
state; immutable width tables live until their paragraph owners are released.
