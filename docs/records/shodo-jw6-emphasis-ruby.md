# shodo-jw6: emphasis outside ruby and nominal overflow

## Cause and decision

After shodo-930, line metrics reserved emphasis independently of ruby. The
retained ruby stack and indexed candidate probes only read font-content bounds,
so taking their maximum allowed same-side marks and readings to overlap. The
paint offset exposed the primary font edge without the reading extent, and
nominal overflow omitted marks entirely.

Keep font-content and annotation-only `whole` bounds separate from the line
contribution. Track geometry places reserved mark outsets beyond the same-side
annotation outer edge and unions the result with existing line contributions.
This reuses leading, leaves the opposite-side offset alone, and counts base
marks once across nested ruby. The same function serves indexed probes and
retained output.

The content index stores two reserved mark outsets from primary font edges,
including edge-window replacements and top/bottom groups. Outsets stay invariant
under profile translation; its selection digest includes them. Existing interval
trees index retained mark outsets and annotation outer edges. Each run queries
only its overlapping base text and measures clearance from its own primary font
edge, including smaller marked text in a larger base column. Outer nested tracks
already enclose child tracks, so queries take the outward extreme, not a sum.
Lines without base emphasis allocate no retained offset vector.
Ellipsis reconstruction preserves offsets of surviving records and gives the
new ellipsis no ruby extent.
An empty offset vector stays empty when truncating an unannotated line. Owned
storage accounting charges offset capacity in each line, including recursive
ruby children, against the completed-line cache budget.

`overflow_rect` includes one square mark em box per eligible typographic
character, or per external combined-text square. Punctuation exclusions,
whitespace, synthetic hyphens and internal combined glyph clusters do not
create extra boxes. The untrimmed paint edge remains distinct from the trimmed
reservation edge; painting may extend beyond line advance.

## Reference

The existing contract targets Chromium 152. This change cross-checked the
current primary Chromium source, without introducing new browser pixel
measurements:

- [`ComputeAnnotationOverflow`](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/core/layout/inline/ruby_utils.cc)
  adds the text item's annotation ascent/descent on the emphasis side.
- [`TextPainter::SetEmphasisMark`](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/core/paint/text_painter.cc)
  includes per-text `AnnotationMetrics` in its untrimmed paint offset.

## Regression evidence

The real CJK fixture's initial same-side Over case exposed offset `27.84` with
no reading clearance. The retained reading contributes `17.390625` in horizontal
layout; the corrected offset is approximately `45.230625`. Vertical tests use
24px bases, 12px readings and 12px marks. One same-side level reserves 48px;
two nested levels reserve 60px, with mark offset 36px. An emphasized plain
sibling keeps offset 12px. A 100px container absorbs both ruby and marks.

A second RED case used a 24px marked run beside 48px text in one base column.
Adding only the reading height left the mark’s inner edge at 12px while the
reading’s outer edge was 0px, overlapping its full 12px extent. Per-run clearance
corrects the mark offset to 36px and reserves 72px; the mark’s inner edge touches
the reading’s outer edge in both vertical modes.

The synthetic overflow regression was also tested with the overflow change
removed: it failed with an empty rectangle instead of the expected mark box
`(inline_start=2.5, block_start=0, inline_size=5, block_size=5)`. Restoring the
change passes; an excluded full stop keeps an empty nominal rectangle because
the synthetic font has no outline ink.

The indexed module checks every nonempty content range for four sibling ruby
containers in horizontal-tb, vertical-rl and vertical-lr, on both emphasis
sides, including mixed font sizes and a top-aligned marked inline, against a
freshly retained line. Eight repeat block probes add at most
eight recorded visits. Retained interval-query visit bounds also cover 64 and
128 emphasized sibling containers. Real-font tests cover same/opposite sides,
nested base ruby, sibling locality, first-line-only emphasis, exact
max-block-size retries, and shared leading. Synthetic tests cover exclusions
and combined squares.

Reproduction uses an isolated Cargo target, `TMPDIR=~/tmp`, and a stable Rust
toolchain. When seeding a target from another worktree, first clean all workspace
packages (`shodo`, `shodo-fixtures`, `shodo-harness`, `shodo-raikiri`,
`shodo-bench`) to avoid Cargo reusing binaries compiled from another checkout.

Independent review identified the missing owned-storage charge for the new
offset vector. A focused RED test reported only 76 owned bytes for a requested
64,956-byte root line. The corrected test verifies unused offset capacity in
both root and ruby-child lines: the last fitting capacity is retained, and one
additional element exceeds the 64 KiB completed-line budget.

## Verification

Final verification uses Rust 1.97.1, an isolated target, and
`CARGO_BUILD_JOBS=2` after cleaning workspace artifacts copied from another
checkout:

- `cargo test --workspace`: 1,602 passed, 8 ignored, no failures across 95
  suite summaries, including examples and doctests. The indexed emphasis/ruby
  parity, emphasized sibling visit bound, and completed-storage capacity
  boundary tests each ran in this final source build.
- `cargo test -p shodo-harness --features accesskit,shodo/complex-scripts --test ruby_geometry`:
  26 real-font tests passed, including the mixed-size same-side regression.
- `cargo test -p shodo --lib emphasis_offset_capacity`: the missing charge
  failed first, then the four root/child and fitting/exceeding cases passed.
- `cargo fmt --all --check` and `git diff --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed after the
  review fix.
- `cargo clippy --workspace --all-targets --features shodo-harness/accesskit -- -D warnings`:
  passed after the review fix.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --features shodo-harness/accesskit`:
  passed after the review fix.

An additional `--all-features` Clippy run reported two pre-existing,
allocation-counting-only bench lints: `needless_question_mark` at
`dev/bench/src/bin/font_match_probe.rs:224` and `bool_comparison` at
`dev/bench/tests/suppressed_warning_format.rs:169`. Those files are unchanged
by this issue; default and accesskit lint gates pass.

## Limits

Overflow is the nominal square em box described by `EmphasisMark`, not a newly
shaped mark outline. A custom mark's ink outside that box, strokes, antialiasing
and other renderer effects remain caller-owned. Existing per-line sizing still
does not borrow leading from neighboring lines. No public API shape changed.

## Follow-up knowledge for shodo-q57

The current public `Line::metrics()` is final: its baseline and advance include
ruby and emphasis contributions. `text_over`/`text_under` exclude marks but are
root font edges, not the full unannotated line-box edges for every inline.
Emphasis already enters the ordinary profile solver before retained ruby
placement, so merely saving the pre-ruby `block_size` would still include marks.

A caller-facing annotation overflow/unused-space API should retain explicit
unannotated line-box edges and annotation reservation edges on each line-relative
side. The shared track geometry has annotation-only `whole` bounds; emphasis
outsets and font-content bounds are kept separate. Capture fixed-point edges
before the final line translation and expose them with the accepted first-line
profile. Map line-over to block-end for vertical-lr. Avoid deriving reusable
leading from `overflow_rect`: that rectangle uses paint edges and glyph ink,
can include the untrimmed-mark difference, and excludes hidden annotation ink
although hidden readings still occupy layout space. Neighbor-line/block policy
belongs to the caller; jw6 continues to size each line independently.
