# Ruby formatting inside shodo's IFC

Issue: shodo-unc.3. Base: dec87fc8b433641383af32f2475dcdb9c4ddcefe.

## Purpose and completion contract

Provide source-preserving ruby in the same incremental IFC as ordinary text.
The caller supplies resolved inline styles and ruby structure; shodo owns
pairing, shaping, coordinated line breaking, geometry and accepted output.
HTML parsing and CSS cascade remain caller responsibilities. This does not
depend on merging or modifying the raikiri integration spike.

All issue requirements are mandatory: base/annotation pairing; breaks between
and within ruby pairs; safe overhang; ruby-align; ruby-position; use of
half-leading and additional leading; horizontal and vertical output. Ruby
must participate in intrinsic measurement, ordinary/balance/pretty layout,
float retries, source mapping, selection and retained glyph rendering.
An unbreakable atomic replacement alone does not fulfill the issue.

The existing workspace baseline is 702 passed, zero failed. Preserve ordinary
non-ruby behavior, public glyph ownership and the original font fixtures.

## Representation options and choice

1. Retain base text in the parent input and attach independently analyzed
   annotation lanes. This preserves the parent source, text transforms and
   hit testing while adding a coordinated formatting layer. Chosen.
2. Nest complete base and annotation paragraphs behind replacement objects.
   This reuses child layout but requires a new hierarchical main-text and
   navigation contract. Annotation lanes may reuse paragraphs; base text
   must remain in the parent's logical source stream.
3. Require caller-sized atomic boxes. This would lose coordinated wrapping
   and overhang; it is not the implementation for this issue.

Ruby normalization, pairing, measurement and placement have separate modules.
Do not enlarge the existing large shape/window files with the complete ruby
algorithm. Existing analysis and line modules call the new formatting layer.

## Public input

Expose a ruby module and re-export its input types. Inputs are immutable and
cloneable through Arc-owned snapshots; snapshots consume ParagraphBuilder
without analyzing it. They retain source ranges, styles, inline markers,
first-line alternatives, limits/errors and warnings. Existing builder calls
remain unchanged.

* RubyContent::from_builder(builder: ParagraphBuilder) -> RubyContent.
* RubyContent::text(source: TextSource, text: &str, style: &InlineStyle,
  limits: &Limits) -> RubyContent is the one-span convenience constructor.
* RubyBase { node: NodeId, content: RubyContent, align: RubyAlign }.
* RubyAnnotation { node: NodeId, content: RubyContent, span: RubySpan,
  visibility: RubyVisibility }.
* RubyLevel { annotations: Vec<RubyAnnotation>, style: RubyStyle }.
* Ruby::new(bases: Vec<RubyBase>, levels: Vec<RubyLevel>) ->
  Result<Ruby, RubyError> validates structural spans without shaping.
* ParagraphBuilder::push_ruby(node: NodeId, style: &InlineStyle,
  ruby: Ruby) -> &mut Self appends one container.
* ParagraphBuilder::push_ruby_with_first_line(node: NodeId,
  normal: &InlineStyle, first_line: &InlineStyle, ruby: Ruby) -> &mut Self
  supplies the same explicit first-line style contract as other inlines.
* RichText::push_ruby(self, ruby: Ruby, style: &InlineStyle) -> Self assigns
  its container node through the existing next-node counter.

RubyAlign is Start, Center, SpaceBetween or SpaceAround (default SpaceAround).
RubyPosition is Over, Under, Alternate, AlternateUnder or InterCharacter
(default Alternate). RubyOverhang is Auto or None (default Auto). RubyMerge is
Separate, Merge or Auto (default Separate). RubyStyle contains align,
position, overhang and merge. RubyVisibility is Visible, Hidden or Collapse
(default Visible). RubySpan is Auto, All or Columns(Range<usize>).

Auto annotations pair in order; absent counterparts receive empty anonymous
boxes. All spans the entire segment. Explicit spans are nonempty, in range
after automatic empty-base normalization, and non-overlapping in one level;
invalid spans return RubyError, never panic or silently change pairing.
Ruby::new validates spans without materializing an expanded pairing table.
Before normalization/import, the parent checks the projected column/item
counts against its limits, so surplus annotations cannot allocate an
unbounded set of anonymous bases before the normal build guards run.
Multiple levels and spanning annotations are supported. An empty container
is valid and must not produce a non-progressing line. Nested ruby in content
uses the same source/output model and shares the nesting limit.

Callers may normalize anonymous HTML boxes to these inputs; they do not have
to calculate widths, break points or pairings. Node IDs in supplied contents
remain caller-owned, exactly as in ParagraphBuilder. Generated empty boxes
have no fabricated source node.

## Normalization and source contracts

Append base snapshots to the parent raw stream with style-index and text-range
remapping. Retain ruby/base boundary markers through whitespace processing
and transforms. Annotation strings never enter Paragraph::text or its primary
OffsetMapping. Base content therefore retains its ordinary UTF-8 and DOM
source anchors, including text transformed into multiple processed scalars.
Each annotation has its own Paragraph, offset mapping and retained fonts.

Force bidi isolation at container/base/annotation boundaries while preserving
the caller's direction and override semantics. Annotation lanes reorder with
their bases, then apply their internal bidi order. Ruby boundaries are shaping
barriers between distinct boxes; style/paint boundaries within a box continue
to obey the existing shared-cluster rules.

Auto-hide an annotation with the same original textContent as its base before
whitespace collapse or transforms when merge is Separate (Auto chooses the
Separate policy). Collapse removes annotation ink and sizing but preserves
pairing; Hidden reserves sizing but emits no paint. Merge disables auto-hide
and treats the level as a spanning annotation. Structural whitespace remains
explicit content in supplied snapshots; parent base whitespace follows its
normal neighboring base-text context, never annotation text.

## Coordinated breaks and measurement

Create a monotone immutable correspondence of safe base-unit cuts and cuts in
every visible annotation lane. Between columns, a cut is available only when
normal base rules allow it and no spanning annotation forbids that cut.
Within a pair, every lane must permit wrapping and have an interior legal
opportunity. Couple opportunities by the nearest proportional typographic
character progress, monotonically, with no empty internal fragment in an
active lane. Equal-distance choices select the earlier opportunity. Preserve
the end cut in every lane. Never cut UTF-8, graphemes, indivisible transforms,
shared shaping clusters or text-combine-upright compositions.

Mandatory breaks coordinate all lanes at safe positions; no-wrap and emergency
opportunities retain their existing CSS precedence. A pair without a legal
parallel cut overflows as one unit. Accepted fragments consume all base and
annotation content exactly once across continuation; a zero-width line still
makes bounded progress. BreakToken remains cheap and retry-safe: its parent
unit cursor identifies immutable lane cuts, not mutable annotation state in
LayoutContext. Alternate first-line data maps cuts by source correspondence.

Measure a candidate fragment from the actual selected base/annotation windows,
including line-edge re-shaping. For Separate, each column's width is the
maximum participating lane width. Distribute additional spanning-annotation
width equally across its columns, processing narrower spans first. For Merge,
measure the merged lane against all spanned bases. Auto uses Separate until a
different policy is explicitly implemented and documented.

Widths belong to candidate/accepted fragments, not permanently stretched
shaper advances or fixed per-character estimates. The same measurements feed
greedy selection, min/max-content, balance/pretty plans and float lookahead.
Spacing and justification outside ruby remain unchanged; ruby alignment is
performed inside the chosen column widths. Caches include ruby identity and
normal/first-line set. Non-ruby paths do not allocate ruby tables.

## Alignment, placement and overhang

Start places content at logical start; Center centers it. SpaceBetween divides
excess width between eligible typographic units; one unit centers.
SpaceAround uses equal internal gaps and half a gap at both ends. Alignment
must account for source-slice glyph ownership and bidi; it never scales glyphs.

Over/Under use line-over/line-under rather than physical block-start/end.
Alternate/AlternateUnder alternate interlinear levels, starting on the named
side. Levels on one side stack outwards. VerticalRl and VerticalLr map the
same line-relative meaning through PhysicalConverter. InterCharacter is an
upright vertical lane next to the horizontal base, and behaves as Over in a
vertical container. Annotation font size comes from supplied resolved style;
there is no hard-coded 25px extent or implicit rescaling of explicit styles.

Overhang Auto permits collision-free extension over adjacent plain text,
bounded by half the annotation's full-width character advance and the
available safe neighbor geometry. Never hang over an atomic box, another
annotation or a line edge. None expands the reserved column width fully.
Calculate safe allowances for the actual candidate/accepted line; do not use
the next line's neighbor. Preserve width agreement between fit, intrinsic
measurement and paint. Punctuation/spacing policies use the base text.

## Line height and overflow

Base boxes participate in baseline and line-height like ordinary inlines.
Annotation font-content bounds are placed outside the base text-over/under
edges. Annotation line-height does not inflate the annotation content box.
Reuse the ruby container's available leading before adding extra leading on
the affected side(s). For identical ruby on successive lines, the resulting
line advance must prevent annotation overlap. Tall annotations and multiple
levels increase the advance as necessary. VerticalAlign displaces base and
associated annotations together; text-box trimming cannot erase annotation
clearance. max_block_size applies to the resulting advance and a retry of
the same token produces the same candidate.

For one baseline-aligned base of font-content height B, container line-height
H, and interlinear annotation stacks of heights O and U, the untrimmed advance
is max(H, B + O + U). The extra max(0, B + O + U - H) is assigned to the
annotation side, or proportionally between O and U when both are nonzero.
For B=20, O=10, U=0: H=40 stays40, H=20 becomes30. An annotation can still
extend outside its current line box; this formula constrains repeat-line
spacing, not containment. Multi-font bases use their actual union of extents.

Line::overflow_rect includes annotation ink and geometry even when it extends
outside the line advance box. Keep line advance and overflow distinct.

## Public output and consumers

Keep base GlyphRun fragments in the parent output. Add Fragment::RubyAnnotation
with a RubyAnnotationView carrying container/base/annotation nodes, level,
base text range, annotation text range, visibility, logical origin and the
retained annotation Line. Expose Line::ruby_annotations as a filtered iterator
and the corresponding Paragraph from the view. Child Line geometry is local;
the view's transform maps it into parent-line coordinates. RubyTransform has
six f32 fields: inline_inline, inline_block, block_inline, block_block,
inline_offset and block_offset. Same-axis lanes use identity plus translation;
horizontal InterCharacter ruby requires an axis conversion. origin returns
(inline_offset, block_offset); transform returns the complete RubyTransform.
Renderer code must traverse the retained child output and apply the transform
once before the parent's PhysicalConverter; it must
not reshape the annotation string or infer a face from its source node.

Main LineLayout navigation and selection retain base reading order and base
sources. Hitting an annotation in the ordinary API resolves to its paired
base. LineLayout::hit_test_ruby returns Option<RubyHit<'_>>; RubyHit contains
annotation: RubyAnnotationView and hit: HitResult, where hit.position and
hit.origin are annotation-local. The retained Paragraph supplies mapping
for callers that edit annotations. Annotation selection uses its
retained child LineLayout. Accessibility preserves base logical reading order
and provides ruby relationships/annotation content as explicit metadata,
without silently duplicating annotations into the main text stream.
AccessibleLayout::ruby_annotations returns an iterator of AccessibleRuby,
with parent_line: usize and annotation: RubyAnnotationView. Its retained child
Line can be passed to AccessibleLayout for annotation-local characters and
sources. This keeps both datasets explicit rather than overloading offsets.

Update all exhaustive Fragment consumers, shared PNG painting, accessibility
and AccessKit exports. Existing outline/color/font-instance/decoration
contracts apply to annotation glyphs. Provide a real-font ruby PNG example
and machine-readable source/geometry output for horizontal and both vertical
writing modes. Existing CJK fixture covers 日本語/にほんご and 読み/よみ.

## Limits, compatibility and verification

Rust 1.89.0, stable, wasm and existing feature combinations remain supported.
No new core dependency is required. All retained base/annotation text, items,
styles and shaped glyphs count against the parent's aggregate Limits,
including first-line alternates and nested ruby. Check budgets before growth;
preserve terminal builder errors, warning caps and bounded nesting. Candidate
measurement must not repeatedly re-shape whole ruby paragraphs or construct
all pairwise cut combinations. Use indexed monotone cuts and bounded existing
edge windows; include a long-ruby operation-count regression.

Verification must include actual sequence glyphs/fonts/source anchors,
short and long pairs, mismatched pairing, multiple levels/spans, empty boxes,
auto-hide/visibility, all alignments and positions, safe/blocked overhang,
half-leading, vertical matrices, coordinated wrapping, retries/continuation,
transforms/ZWJ/ligatures, bidi, first-line, intrinsic/planned layout, aggregate
limits, hits/selection/accessibility and real PNG output. Existing snapshots
must remain unchanged. Complete required workspace, feature, MSRV, wasm,
fixture-tool and example gates and one fresh whole-branch review before PR.

## Sources and authority

Normative design reference: [CSS Ruby Layout 1](https://www.w3.org/TR/css-ruby-1/),
especially pairing, coordinated wrapping, line spacing and edge effects.
Browser behavior comparison: [Chrome's line-breakable ruby](https://developer.chrome.com/blog/line-breakable-ruby).
Browser heuristics and pixel compatibility are comparison targets; shodo's
documented public behavior and full issue requirements remain the acceptance
contract. The existing foundation specification section 6.3 also requires
separate annotation overflow and line advance.

This specification is author-reviewed under the user's autonomous issue
implementation instruction; it is not represented as a human-reviewed spec.
