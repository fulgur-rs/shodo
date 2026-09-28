# Reuse paragraph grapheme cuts during itemization

## Outcome and constraints

Implement shodo-j2r.5: select the sorted `BreakAnalysis.graphemes` cuts
with `partition_point` and eliminate ICU segmentation of each flushed
shaping string. Preserve font cluster queries, script/style subdivision,
orientation, scalar source ranges, and grapheme-start/caret contracts for
every supported input. Normal runs must consume shared cuts without a
second Unicode property pass. Existing source-cursor segmentation is a
different input contract and is outside this change.

No public API or dependency changes. Keep Rust 1.89 compatibility and all
feature combinations. Do not alter existing font, browser, or WPT fixtures.
The raikiri integration spikes and shodo-p2m.6 remain outside this work.

## Why slicing alone is insufficient

Actual builder characterization establishes these local font queries:

* Padding between `🇦` and `🇧🇨🇩`: `🇦`, `🇧🇨`, `🇩`.
* Padding between `👩` and `ZWJ💻`: `👩`, `ZWJ`, `💻`.
* Padding between `क` and `्क`: `क`, `्`, `क`.
* Authored `🇦LRM🇧🇨🇩`: `🇦`, `LRM`, `🇧🇨`, `🇩`.
* Vertical text-combine-upright `ガ12`: `ｶﾞ`, `1`, `2`, with source
  scalar ranges `(0,3), (0,3), (3,4), (4,5)`.

Whole-paragraph segmentation crosses decoration boundaries and omits bidi
controls. Local shaping segmentation restarts at hard boundaries and
includes authored controls. Regional-indicator parity, emoji ZWJ context,
and Indic linker context can consequently differ. An upstream cut before
a transparent source gap also cannot be used as a local byte index.

## Selected design

Retain ICU as the paragraph segmentation authority. A private
`analysis/itemize/graphemes.rs` helper returns scalar-index cuts for a
flush; itemize maps those cuts through its existing local UTF-8 offsets
when selecting font clusters.

1. Use two `partition_point` calls to select shared source cuts between
   the run's first scalar offset and last scalar end. Map each source cut
   to `scalars.partition_point(|s| s.end <= cut)`. Force run edges, dedupe
   equal scalar cuts, and preserve ordering. This handles transparent
   source gaps and repeated ranges from Kana width reversion.
2. Record authored bidi-control offsets while the existing break
   projection already classifies its input. Reuse that sorted metadata;
   do not scan every run again to discover controls. Merge the controls'
   before/after scalar cuts with shared cuts. Generated bidi-control
   items already flush and do not enter local shaping strings.
3. At run start and after an authored control, consult the sorted
   `typographic_starts`. A restart at a paragraph grapheme start needs
   no repair. Otherwise, replace only the prefix through the next shared
   cut with cuts evaluated from a fresh local UAX29 context. For an
   initial RI sequence, extend this repair to the first shared cut at or
   beyond the end of that RI sequence, because odd parity can shift all
   subsequent RI pairs. An authored control bounds each repair region.
4. The private prefix evaluator applies UAX29 GB3–GB13 using existing
   ICU GCB, IndicConjunctBreak, and ExtendedPictographic property data.
   Adjacent rules are included to preserve their precedence over the
   long-context rules. It never runs on normal complete graphemes, never
   replaces paragraph segmentation, and never invokes an ICU segmenter.
   Merge disjoint repaired prefixes with untouched shared cuts linearly;
   avoid repeated Vec splices and quadratic behavior with many controls.

The rule reference is Unicode 17 UAX29 revision47:
https://www.unicode.org/reports/tr29/tr29-47.html#Grapheme_Cluster_Boundary_Rules.
The linked ICU property and segmenter data are from the same dependency
family. Differential tests must guard their agreement on upgrades.

Width reversion is safe to project when changed forms preserve GCB,
InCB, and EP properties, except voiced Kana's Other+Extend expansion.
An exhaustive test over every actual width-table entry and fullwidth
ASCII verifies this assumption; expanded Kana's first scalar must also
be InCB=None and EP=false. Width origins undo width-only substitutions
after case/kana transforms; NFC Kana composition preserves grapheme
cuts, and generated expansion scalars stay together during run assembly.

Alternatives rejected: unconditional cut slicing changes the established
queries above; conditional full-run ICU fallback leaves the required
duplicate pass; implementing an independent paragraph segmenter adds
unnecessary Unicode maintenance and discards the shared result.

## Evidence required

* Real builder font-query and orientation characterization for the five
  cases above, and preservation of width properties across the entire
  mapping table.
* Runtime RED→GREEN showing local ICU boundary visits disappear for
  ordinary paragraphs and many decorated runs; the observation must
  wrap the actual old iterator, not count intended calls.
* Unicode GraphemeBreakTest17 normative cases, with every scalar-aligned
  substring compared to ICU's segmentation of that exact substring.
  Include the unmodified normative boundaries for whole-input checks.
* Independent generated combinations across GCB/InCB/EP classes and
  long RI/emoji/Indic context chains. Include source gaps and inserted
  authored bidi controls, comparing the helper to local ICU.
* Existing whole workspace tests, AccessKit, core-only/complex-scripts,
  release library tests, formatting, clippy, and warning-free docs.
* Fresh whole-branch review; exact PR HEAD check/msrv/wasm success;
  actual merged tree verification; issue close and owned cleanup.

No wall-clock or allocation improvement is claimed from counters alone.
Prefix repair can touch a long truncated grapheme/RI chain; that work is
necessary to preserve local context and is measured separately from
removed redundant whole-run ICU work.
