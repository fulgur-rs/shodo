# raikiri paged layout: width, body margin and widow/orphan contract (shodo-tmr / shodo-ua8)

Status: on 2026-09-30 the user chose Chrome as the body-margin reference.
Classes (a) and (c) below are raikiri native mismatches, superseding the
original classification as caller defects. The caller must not suppress UA
body margins to reproduce native's current behavior. No raikiri or caller
fix has been made; the preserved S4v2 spike must not be changed. Original
native/candidate evidence (not rerun):
`target/8ei-artifacts/pagination-all-pages-validation/` (gitignored; from
shodo-8ei, S4v2 `fe67a28`, pinned raikiri `ab7e619`, 800px wide, heights
128/64/32/128). Of 242 attempts, 19 pairs succeeded on both sides and 0 are
eligible for a like-for-like speed comparison. Line references below are
from the investigation and were spot-checked for (a), (b), (c) and (d).

## Classes (19 paired successes; a document can be in several)

| Class | Docs | Cause | Owner |
|---|---|---|---|
| (a) auto content width 784 vs 800 | 15 (7 differ only here) | Candidate subtracts the UA body margin 8+8, matching Chrome; native zeroes UA-origin body sides in paged mode (`raikiri-dom/src/layout/page_pipeline.rs:253-277`) | raikiri native (shodo-ua8) |
| (b) `ch` 160.000305 vs 160 | 5 | `FontCollection::resolve_ch` returns skrifa's 16.16 scaled advance: 2^-15 px per `ch`. shodo's own shaped advances are exact (16.0), so it is inconsistent | shodo core |
| (c) UA body block-start margin | 6 | Candidate seeds the pending margin with 8; native uses 0 unless the side is authored or body has direct text with a canvas background (`page_pipeline.rs:1058-1084`). Only visible when the first child is a `<div>` (with a `<p>`, max(8,16) hides it). Native's UA-only suppression is the mismatch; margin collapse and fragmentation still need Chrome comparison | raikiri native (shodo-ua8) |
| (d) widow/orphan on an oversized IFC | 2 (c000, c055 at 32px) | Native's pass excludes oversized or nested IFCs (comment at `page_pipeline.rs:881-884`); the candidate applies orphans/widows to every fragment (CSS Break 3 §3.3) | native semantic difference |
| (e) margin adjoining an unforced break | 3 (c000, c055, c120) | Native carries the margin across the break and jumps pages; the candidate truncates only by accident (height rejection then `lines < orphans`), and counts it as `LookaheadRejected` | native semantic difference; candidate counters dishonest |
| (f) harness projection | 1 (c069) | Native projection partitions per text node, the candidate per IFC | harness |

Partition mismatches: 11 documents = 6 (c) + 2 (d) + 3 (e), plus c097 as a
knock-on of (a) (a 51-char line fits at 800 but wraps at 784) and c069 as
(f), counted within those totals as reported. The original prediction that
8 of the 11 would become comparable assumed changing the caller to match
native. It is superseded: rerun after the native correction before claiming
any comparable pairs.
c000, c055 and c120 remain excluded as real semantic differences.

## Chrome reference (2026-09-30)

Chromium 152.0.7977.82 (Arch Linux) was tested with this standards-mode input:

```html
<!doctype html>
<style>
  @page { size: 800px 400px; margin: 0; }
  div { font: 16px monospace; }
  span { float: right; }
</style>
<div>LEFT<span>RIGHT</span></div>
```

Print with `chromium --headless --no-pdf-header-footer
--print-to-pdf=/tmp/body.pdf file:///tmp/body.html`, then inspect with
`pdftotext -bbox /tmp/body.pdf -`. Repeat with `body { margin: 0; }` and
`body { margin: 8px; }` inserted into the style block. The PDF page width was
600pt (800 CSS px); the left word started at 6pt (8px) for both the UA
default and explicit 8px, and at 0pt for explicit zero. Right-word positions
also moved by 6pt, consistent with 784px versus 800px available width.
The UA default and explicit 8px produced identical word bounds. This is a
single-page print check, not a validation of every fragmentation case.

Primary sources:

- [Blink UA stylesheet](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/core/html/resources/html.css): `body { margin: 8px; }`, not restricted to screen media.
- [CSS Paged Media page model](https://www.w3.org/TR/css-page-3/#page-model): page margins belong to the page box; setting `@page` margins to zero does not reset body margins.

## Contract for native and caller

1. In paged mode, use the cascaded body margins, including UA-origin sides.
   Authored declarations override them through the normal cascade; do not
   zero sides based on `CascadeResult::non_ua_margin_sides`. For an ordinary
   auto-width body without padding or borders, content width = page content
   width minus the used inline margins (800 - 8 - 8 = 784px by default;
   800px with authored `body { margin: 0; }`). Block margins follow normal
   collapse and fragmentation behavior, with Chrome as the reference.
   Native's direct-text / canvas-background exception is not a contract to
   copy: verify those cases against Chrome when fixing native. Original
   HTML/CSS is never changed to make comparison outputs agree.
2. `ch` must be linear and consistent with shaped advances (shodo core).
3. Comparison is per IFC, merging native text-node ranges.
4. Oversized-IFC widow/orphan and unforced-break margin truncation stay
   documented intentional divergences; such pairs are excluded from speed
   ratios and success counts, or run with a harness mode of orphans=widows=1
   for blocks taller than the page (a harness setting, not a CSS change).

## Production switch (shodo-p2m.6)

Required: contract items 1 and 2 (native currently gives the ordinary
auto-width body 16px too much width and shifts wrapping/pages; `ch` blocks
exact-fit breaks). Item 1 is a raikiri native correction tracked by
shodo-ua8, not a caller workaround. Not required to
replicate native: (d) and (e). Item 3 is harness only.

## Not done

The native correction and Chrome comparisons for authored per-side margins,
first-child margin collapse, direct text / canvas background, and multipage
fragmentation are still pending. fresh / reentry / restored-height, source
completeness and allocator ownership checks, and any speed re-comparison,
need the corrected native and the adopted production caller. No pair is
claimed comparable. Performance measurements inform improvement priorities;
unfinished measurement or a speed budget does not block the production
switch (user decision recorded in shodo-p2m.6 on 2026-09-30).
