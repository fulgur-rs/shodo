# shodo-q57: caller-visible annotation overflow and space

## Design

Expose `Line::annotation_metrics() -> AnnotationMetrics` with unannotated
logical block-start/end coordinates relative to the accepted line, and nonnegative
line-over/under annotation overflow and unused leading. The unannotated box is
the same selected content's line profile with emphasis sizing disabled, aligned
to the accepted dominant baseline. It is independent of paragraph root font
edges, supports negative leading, and is translated with retained ruby.

Unused leading considers actual accepted text/atomic displacements, trimmed
font-content bounds, retained layout annotation edges, and reserved emphasis
edges. Ordinary font content extending past a short box does not become
annotation overflow and prevents reusable space on that side. Actual annotations
can still expose their own overflow there. Space stays within the intersection
of the accepted and independently rounded unannotated boxes.
Values describe layout reservations, not painted ink or nominal mark boxes.

The library keeps standalone line advancement. A caller can subtract
`min(previous.space_under, next.overflow_over)` when these sides meet and both
lines use the same writing mode and coordinate units; reverse
the line-relative sides for vertical-lr. Blocks supply their own context and
policy. No neighboring line, parent block, renderer, or browser is consulted.

Only accepted lines need the emphasis-free profile. Candidate range indexes
and probe traversal stay unchanged. Retained geometry consists of fixed-size
scalars charged through the existing line header, with no new owned vectors.

## Existing reproduction

The protected `emfix.html` scratch input uses a 10px `latin.ttf` face, dot
emphasis, 4px line-height, and `ab<br>ab`; it also includes preceding blocks.
The scratch font matches the fixed Latin fixture (SHA-256
`7aa5c6687e9a8b72f71ea5abaded28d771ecb54c66a8a9ac49268537b2f94d25`):
units-per-em 1000, hhea ascent/descent 1069/-293. The reported Shodo 24px is an
integer approximation, not a universal two-line advance. Chromium's whole-pixel
font rounding and preceding annotation space also affect comparisons. This
change creates no new browser comparison results.

## Verification

Initial missing-API regression failed with E0599, then eight synthetic contract
tests passed. Mixed sibling regressions first failed: ordinary large-font content
inflated annotation overflow from 5px to 8px; a centered root em floor erased
displaced small marks (15px space instead of 11.5px); a noncontributing 100px
quirks root erased a small annotation's 2.5px overflow. Annotation-only outer
edges, union with a contributing root floor, and the selected solver's root
participation flag made all three pass.

The fixed-font fractional-leading regression failed with 0.203125px available
space where only 0.1875px fit the accepted line. The independent bare-profile
rounding is preserved, while reusable space is clipped to the accepted box.
Final complete verification will be recorded below.

## Implementation rulings

- Preserve the separately quantized emphasis-free solver's exact result. The
  Latin 4px case has a 4.015625px bare advance after existing floating-point
  cancellation and outward rounding; aligning it to the accepted baseline
  places its block-end 1/64px beyond the annotated advance. Side values are
  not guaranteed to sum back to final advance. This costs a small coordinate
  distinction, and avoids misrepresenting the actual unannotated profile.
- Reuse the selected solver's root-strut participation result rather than
  rescanning quirks candidates or inventing a second participation rule.
- Chromium's [annotation overflow calculation](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/core/layout/inline/ruby_utils.cc)
  separates layout font-content from ink, reserves annotation edges and returns
  per-side overflow/space. Its [inline layout caller](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/core/layout/inline/inline_layout_algorithm.cc)
  borrows the minimum of the meeting sides. This API exposes local inputs while
  leaving neighboring-line policy with the caller.
