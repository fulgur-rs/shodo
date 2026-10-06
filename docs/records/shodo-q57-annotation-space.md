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

After integrating main's public `force_root_strut`, synthetic tests cover its
false/true behavior, top/bottom-aligned groups, atomic over/under reservations,
and combined text in both vertical modes. A tab-only combined square regression
failed with zero overflow where its accepted profile reserved a 5px mark;
including retained external squares without requiring glyph records made it
pass. The complete synthetic suite now has 13 tests. Four fixed-font tests cover
the original dimensions, fractional leading, hidden/collapsed/opposite ruby,
and nested root/child geometry.

The 64/128-sibling resource test observes one accepted annotation scan per root
or child fragment; 128 repeated public queries add no scans. A rejected-line
test confirms two retries and acceptance move cached vectors and preserve
geometry without scanning content again. Fixed geometry is charged in the
existing root/child `Line` headers; the existing 64KiB capacity boundary test
continues to cover retained storage.

Independent review identified three additional compositions, all reproduced
before their fixes: ruby's global translation left a tab-only combined square
unmoved and lost 5px of under-side overflow; a quirks-trimmed trailing 40px space
restored excluded metrics (29px phantom overflow and 9px under space instead of
0px and 15px); valid strongly asymmetric font metrics offered 13px of leading
inside a 4px line. Retained squares now move with the baseline, the selected
solver passes its trailing boundary to the shared trimming predicate, and
reusable space is capped to the full intersection length.

The initial full workspace run on `9db9088` stopped at the library suite:
735 passed, one failed, eight ignored. The failure was the existing atomic
baseline lookup resource bound: the annotation scan repeated a lookup already
done by metrics. Atomic records now resolve and retain their baseline kind once
at construction for metric probes, retained geometry and output views to share.
The existing unchanged focused bound then passed. This failed run is not final
verification of the corrected source.

Main `132696d` (styled forced breaks) was merged with published history preserved.
The public export and profile-resolver conflicts preserve both APIs and both
profile paths. A six-case matrix combines a marked 5px child with an independently
styled 40px break in horizontal/vertical-rl/vertical-lr and forced-root false/true.
The bare 40px strut survives; adding emphasis to the text-free break itself adds
no annotation reservation. The 15-test annotation suite and existing 25-test
quirks suite pass after this integration.

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

## Final validation

The reviewed runtime source is `0512131`, based on main `132696d`; independent
source review reported no remaining Critical, Important or Minor findings. The
final test-literal spelling `14. + 1. / 256.` preserves the exact f32 boundary
while satisfying Clippy's excessive-precision lint. Runtime source is unchanged
after that review.

The dedicated-target default workspace run passed 1654 tests in 98 suites with
zero failures and nine ignored. Eight are existing library diagnostic/performance
probes; the ninth is main's opt-in
`original_wpt_fonts_preserve_normal_and_match_explicit_trim_all`, which requires
original WPT fonts and `SHODO_181_WPT_ROOT`. No q57 regression is ignored.
The log contains rebuilt workspace sources and all q57 public, capacity, cache,
atomic lookup and scaling regressions.
The accesskit-enabled workspace run passed 1670 tests in 99 suites, again with
zero failures and the same nine ignored; it includes the final boundary-test
expression.

Workspace all-targets Clippy with warnings denied passed for default and
`shodo-harness/accesskit`. Workspace rustdoc with warnings denied passed for
both configurations. Package listing contains 197 paths, all inside the CI
allowlist; formatting, diff whitespace and machine-home-path checks passed.
The public annotation suite passed 15 tests each with no default features and
with only `complex-scripts`; the same-value fractional-leading regression also
passed after its literal-spelling change.

Reproduction uses Rust 1.97.1, jobs 2, `TMPDIR=~/tmp` and a dedicated Cargo target.
Before using the copied third-party cache, all five workspace packages were
cleaned to force recompilation from this worktree:

```sh
export TMPDIR="$HOME/tmp"
export CARGO_BUILD_JOBS=2
export CARGO_TARGET_DIR="$HOME/tmp/shodo-q57-goal-target"
cargo clean -p shodo -p shodo-fixtures -p shodo-harness -p shodo-raikiri -p shodo-bench
cargo test --workspace
cargo test --workspace --features shodo-harness/accesskit
cargo test -p shodo --no-default-features --test annotation_metrics
cargo test -p shodo --no-default-features --features complex-scripts --test annotation_metrics
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features shodo-harness/accesskit -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --features shodo-harness/accesskit
cargo fmt --all --check
git diff --check
```

After transcribing results and reproduction conditions, all 34 task-owned raw
logs/package listings were removed. The isolated worktree and
`~/tmp/shodo-q57-goal-target` remain for the parent's PR/CI handoff and subsequent
cleanup. The original scratch reproduction, other worktrees, original font
assets and other tasks' outputs were preserved.
