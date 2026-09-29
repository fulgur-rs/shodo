# raikiri paged caller: width, body margin and widow/orphan contract (shodo-tmr)

Status: cause classification done from saved evidence and primary sources.
No fix was made and nothing was rerun: the caller lives in the unmerged S4v2
spike, which must not be changed. Evidence:
`target/8ei-artifacts/pagination-all-pages-validation/` (gitignored; from
shodo-8ei, S4v2 `fe67a28`, pinned raikiri `ab7e619`, 800px wide, heights
128/64/32/128). Of 242 attempts, 19 pairs succeeded on both sides and 0 are
eligible for a like-for-like speed comparison. Line references below are
from the investigation and were spot-checked for (a), (b), (c) and (d).

## Classes (19 paired successes; a document can be in several)

| Class | Docs | Cause | Owner |
|---|---|---|---|
| (a) auto content width 784 vs 800 | 15 (7 differ only here) | Candidate subtracts the UA body margin 8+8; native zeroes UA-origin body sides in paged mode (`raikiri-dom/src/layout/page_pipeline.rs:253-277`) | caller |
| (b) `ch` 160.000305 vs 160 | 5 | `FontCollection::resolve_ch` returns skrifa's 16.16 scaled advance: 2^-15 px per `ch`. shodo's own shaped advances are exact (16.0), so it is inconsistent | shodo core |
| (c) UA body block-start margin | 6 | Candidate seeds the pending margin with 8; native uses 0 unless the side is authored or body has direct text with a canvas background (`page_pipeline.rs:1058-1084`). Only visible when the first child is a `<div>` (with a `<p>`, max(8,16) hides it) | caller |
| (d) widow/orphan on an oversized IFC | 2 (c000, c055 at 32px) | Native's pass excludes oversized or nested IFCs (comment at `page_pipeline.rs:881-884`); the candidate applies orphans/widows to every fragment (CSS Break 3 §3.3) | native semantic difference |
| (e) margin adjoining an unforced break | 3 (c000, c055, c120) | Native carries the margin across the break and jumps pages; the candidate truncates only by accident (height rejection then `lines < orphans`), and counts it as `LookaheadRejected` | native semantic difference; candidate counters dishonest |
| (f) harness projection | 1 (c069) | Native projection partitions per text node, the candidate per IFC | harness |

Partition mismatches: 11 documents = 6 (c) + 2 (d) + 3 (e), plus c097 as a
knock-on of (a) (a 51-char line fits at 800 but wraps at 784) and c069 as
(f), counted within those totals as reported. After (a), (b), (c), (f) are
fixed, 8 of the 11 are expected to become comparable (inferred, not rerun).
c000, c055 and c120 remain excluded as real semantic differences.

## Contract the caller must own

1. In paged mode, body sides whose margin comes from the UA sheet are 0;
   authored sides are kept (`CascadeResult::non_ua_margin_sides`,
   `raikiri-style/src/cascade.rs:67-73`), with native's direct-text /
   canvas-background exception. Content width = page content width minus
   authored margins. Original HTML/CSS is never changed.
2. `ch` must be linear and consistent with shaped advances (shodo core).
3. Comparison is per IFC, merging native text-node ranges.
4. Oversized-IFC widow/orphan and unforced-break margin truncation stay
   documented intentional divergences; such pairs are excluded from speed
   ratios and success counts, or run with a harness mode of orphans=widows=1
   for blocks taller than the page (a harness setting, not a CSS change).

## Production switch (shodo-p2m.6)

Required: contract items 1 and 2 (otherwise every auto-width block wraps 16px
narrower and pages shift; `ch` blocks exact-fit breaks). Not required to
replicate native: (d) and (e). Item 3 is harness only.

## Not done

fresh / reentry / restored-height, source completeness and allocator
ownership checks, and any speed re-comparison, need the corrected caller and
therefore S4's adoption decision. No pair is claimed comparable.
