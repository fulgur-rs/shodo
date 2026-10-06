# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Add `ParagraphBuilder::push_forced_break_with_style` for an independent
  break strut, and `Line::forced_break` to read its node and effective style.

## [0.0.18](https://github.com/fulgur-rs/shodo/compare/v0.0.17...v0.0.18) - 2026-10-06

### Added

- add Line::truncate_with_ellipsis for text-overflow: ellipsis
- reserve line-box extent for emphasis marks
- add first_letter_range for ::first-letter text
- report the color glyph formats a face carries

### Fixed

- harden ellipsis truncation for spacing, boxes and long lines
- mark emphasized atomics and trim fallback runs to their em box
- lay out emphasis marks as annotation overflow like Chromium 152
- match Chromium for punctuation-only text and raw input
- read color record arrays and document what the formats skip

## [0.0.17](https://github.com/fulgur-rs/shodo/compare/v0.0.16...v0.0.17) - 2026-10-06

### Fixed

- match Blink pending vertical-align credit for br struts

### Other

- Merge pull request #236 from fulgur-rs/fix/shodo-9kt-pending-vertical-align

## [0.0.16](https://github.com/fulgur-rs/shodo/compare/v0.0.15...v0.0.16) - 2026-10-06

### Added

- add the quirks-mode line strut predicate
- add ParagraphStyle::line_height_quirk
- bound ruby line measurement work per layout call

### Fixed

- apply the quirks-mode strut rule to ruby metric probes
- suppress quirks-mode struts per line in retained metrics
- charge restarted ruby walks and exempt the float placement probe
- keep ruby range caches for both intrinsic atomic revisions
- cap the ruby block size caches
- charge ruby range cache hits like cold measurements

### Other

- pin DI as a known vertical-align difference
- move ruby quirk parity tests into their own module
- pin remaining quirks matrix cases and the ruby root strut
- narrow quirk helpers and assert the trailing group bound
- compute vertical-align group deltas once
- pin shifts of suppressed and ghost vertical-align groups
- rewrap the max_ruby_line_work field docs
- stash range caches only once atomic revisions alternate
- Merge pull request #232 from fulgur-rs/fix/shodo-tj5-cache-free-charges
- pin refused block replays against a cache-free context

## [0.0.15](https://github.com/fulgur-rs/shodo/compare/v0.0.14...v0.0.15) - 2026-10-05

### Added

- apply hanging-punctuation per inline box

### Fixed

- keep ruby annotation lanes free of per-run hanging
- keep ruby measurement free of per-run hanging
- keep run-byte splits before authored bidi controls
- flag grapheme starts after transparent gaps

### Other

- cover per-run allow_end and root and first-line hanging
- document hanging-punctuation flags
- pin the bidi control flag after projected content
- cover a mixed tab prefix and make the golden update fail loudly
- run the alternating tab starts check under tab-driven invalidation
- pin linear ruby measurement and exact replay with preserved tabs
- move the range cache generation only on tab steps with saturation
- record tab range widths and saturation before shodo-b7d

## [0.0.14](https://github.com/fulgur-rs/shodo/compare/v0.0.13...v0.0.14) - 2026-10-05

### Fixed

- classify dot and colon punctuation from the shaped glyph
- measure upright vertical punctuation blanks along the y axis
- bound nested ruby accumulator slots and pin reverse composition
- keep the incremental ruby walk fail-closed on cut placement

### Other

- drop a redundant accumulator reset and test-only replay gate
- report sibling ruby operation counts for the shodo-2j6 record
- cover sibling ruby accumulator paths against the reference
- answer clipped ruby ancestors' descendant reads from the accumulator tree
- re-measure ruby containers only for profile and edge window changes
- re-measure only ruby containers next to newly selected units
- measure ruby candidates through the container accumulator
- add the ruby container accumulator and its segment tree
- detach shared ruby profile effects and note container dependencies
- measure ruby containers one at a time behind a descendants view
- add effect sequencing, repetition and frame-targeted replay
- split the ruby range cache generation into fills and epoch
- Merge pull request #223 from fulgur-rs/fix/shodo-d77-deep-ruby-break
- document the ruby memo bounds, mid-operation reset and nested wall-clock cause
- assert distinct memo keys for intrinsic atomics
- bound the ruby through memo to look-ahead probes and release its capacity
- add shodo-d77 ruby depth probe
- sharpen ruby memo reach assertions
- resume the ruby look-ahead walk for growing probe ends (shodo-d77)
- memoize ruby candidate cores per operation by look-ahead endpoint (shodo-d77)
- share one selected ruby line profile per candidate with exact replay (shodo-d77)
- keep ruby metric and neighbor indexes in value slots (shodo-d77)
- cover float, forced-break and clear points in ruby memo harness
- add reference path and D^2 guards for ruby candidates (shodo-d77)

## [0.0.13](https://github.com/fulgur-rs/shodo/compare/v0.0.12...v0.0.13) - 2026-10-05

### Fixed

- guard proportional quote cluster classification
- classify proportional closing quotes for pairing

## [0.0.12](https://github.com/fulgur-rs/shodo/compare/v0.0.11...v0.0.12) - 2026-10-04

### Added

- space UTR #59 conditional punctuation for Chinese autospace (shodo-bor)

### Fixed

- prevent text-autospace across U+200B (shodo-6vb)
- remove needless borrows in TCY probe fixtures

### Other

- Merge pull request #219 from fulgur-rs/feat/shodo-9ye-word-spacing-percent
- Merge pull request #218 from fulgur-rs/perf/shodo-t3t-tcy-probe-reuse
- record empty combine span scan measurements
- push missing-font notdef glyphs without temporary runs
- pin CFF origin cache to one shaping input
- prepare shaping face state once per input
- keep shaping window contexts inline
- measure ruby content restoration scans
- measure TCY width probes under ruby scopes
- Optimize ruby alignment group and gap storage
- share first-line whitespace flags
- lazily format suppressed warning messages
- reuse StyleMetrics probe instances
- skip Ruby base ancestor Vec scans
- Merge remote-tracking branch 'origin/main' into docs/rustdoc-guides
- Merge pull request #205 from fulgur-rs/docs/rustdoc-lines
- Merge pull request #204 from fulgur-rs/docs/rustdoc-builders
- explain paragraph builder input contracts
- borrow Ruby base caret stop ranges
- avoid ruby hit paths in regular hit testing

## [0.0.11](https://github.com/fulgur-rs/shodo/compare/v0.0.10...v0.0.11) - 2026-10-03

### Other

- avoid bidi text copy without line separator
- reuse combined width trial shapes
- reduce style metrics cache miss cost
- reuse metrics across paint-only styles
- share identical paragraph font metrics
- streamline paint geometry cluster scans ([#190](https://github.com/fulgur-rs/shodo/pull/190))
- cover empty combined geometry fallback
- streamline paint geometry cluster scans
- adapt ruby index split axis
- index ruby hit bounds
- Index font matching candidates
- use cursors for grapheme itemization

## [0.0.10](https://github.com/fulgur-rs/shodo/compare/v0.0.9...v0.0.10) - 2026-10-02

### Fixed

- keep default-ignorable characters invisible
- budget ruby cut search before traversing candidates
- resolve accessible glyph owners with a monotone cursor
- index variation settings without quadratic scans
- share shaping features across paragraph and line edges
- charge GDEF mark sets to cumulative work budget

### Fixed

- Keep default-ignorable characters invisible through font fallback and shaping, while preserving their logical text, source clusters, and discretionary hyphens.

## [0.0.9](https://github.com/fulgur-rs/shodo/compare/v0.0.8...v0.0.9) - 2026-10-02

### Other

- Merge remote-tracking branch 'origin/main' into perf/t6q-6-decoration-ancestors
- Merge pull request #175 from fulgur-rs/perf/t6q-9-cluster-ends
- Merge pull request #174 from fulgur-rs/perf/t6q-4-paint-geometry
- Merge remote-tracking branch 'origin/main' into perf/t6q-4-paint-geometry
- *(paint)* build shared geometry without navigation indexes
- Merge pull request #169 from fulgur-rs/perf/t6q-1-ruby-hit
- Merge remote-tracking branch 'origin/main' into perf/t6q-1-ruby-hit
- avoid repeated nested ruby hit searches

## [0.0.8](https://github.com/fulgur-rs/shodo/compare/v0.0.7...v0.0.8) - 2026-10-02

### Added

- *(font)* expose family name for FontId (shodo-9se)
- support word-space-transform in inline styles
- expose inline box baseline

### Fixed

- *(font)* ignore empty preferred family names
- preserve paragraph advance contract for hanging punctuation
- include leading hanging punctuation in inline size
- preserve word-space transforms in text combine

### Other

- Preserve warning order after context-free analysis
- Analyze first-line data before shaping
- Split paragraph analysis from shaping
- clarify object replacement text contracts
- satisfy clippy in word-space-transform cases
- Test slice offsets with RTL inline boxes
- Expose slice coordinates for inline box fragments
- Merge pull request #155 from fulgur-rs/feat/20v-inline-box-baseline

## [0.0.7](https://github.com/fulgur-rs/shodo/compare/v0.0.6...v0.0.7) - 2026-10-01

### Added

- *(style)* add word-break break-word
- *(output)* expose inline box start edge direction
- add paragraph width measurement methods
- *(font)* expose shared and document generations for reuse
- expose glyph sources and line DOM owners
- *(output)* expose physical glyph outline origins
- *(line)* construct constraints from physical insets
- *(line)* expose complete trailing whitespace advance
- *(font)* expose bundled-only discovery policy

### Fixed

- *(line)* apply each-line indent after block-in-inline
- *(line)* justify Unicode word separators consistently
- *(line)* keep disabled justification at line start
- retain MSRV-compatible atomic update on Rust 1.99
- *(output)* trim collapsible spaces from inline box widths
- *(text)* complete Unicode full-width mappings
- *(font)* keep native discovery outside registration budgets
- *(font)* stabilize native fallback across lazy materialization

### Other

- allow concurrent font cache hits

### Fixed

- Honor `text-indent: each-line` after block-in-inline boundaries in line
  layout, intrinsic sizing and Pretty wrap planning.

- Include NBSP and other Unicode word separators in word justification so
  preserved space runs align like their nonbreaking-space equivalents.

- Keep `text-justify: none` at line start when alignment requests justification,
  including `text-align-last: justify`, while retaining explicit end/center alignment.

- Allow concurrent font-cache hits across shared collection clones, retaining
  bounded LRU caches without waiting for font catalog access.

- Exclude collapsible line-end spaces from inline background and border widths;
  keep preserved hanging spaces inside their boxes, including after bidi resets.

- Complete `text-transform: full-width` Unicode mappings for halfwidth Hangul,
  currency symbols, white parentheses, arrows and shapes.

- Exempt lazily materialized platform faces from per-layer registration budgets,
  while preserving individual font validation and explicit CSS source limits.

- Keep font fallback independent of lazy platform-face materialization order;
  platform faces no longer enter the explicitly registered last-resort list,
  and equal-ranked native sources retain their catalog identity ordering.

### Added

- `FontCollection::generations()` exposes shared/document counters for reuse
  keys, including shared generic and fallback configuration changes.

- `GlyphRunView::source()` reports caller text origins and `Line::owners()`
  lists DOM source ranges without scanning fragments separately for each node.

- `GlyphRunView::physical_origin()` returns container-relative outline positions
  with line offsets, RTL shaping advances and vertical writing modes resolved.

- `LineConstraint::from_physical_insets()` converts physical float insets
  into the available inline size and logical start offset in all writing modes.

- `Line::trailing_whitespace()` exposes trailing space and tab advances,
  including both retained and hanging whitespace; clarify `hang_end()` docs.

- `FontCollection::is_bundled_only()` exposes the system-font discovery policy,
  including the shared-root policy inherited by document layers.

## [0.0.6](https://github.com/fulgur-rs/shodo/compare/v0.0.5...v0.0.6) - 2026-10-01

### Fixed

- invalidate completed trials before intervening context work
- *(ruby)* check aggregate text budget before normalization
- *(font)* reject malformed variation axis ranges

### Other

- Merge pull request #134 from fulgur-rs/perf/c91-5-balance-summary
- stream Balance endpoints and publish aggregate results
- Merge pull request #132 from fulgur-rs/perf/c91-4-height-retry
- bound retry ownership and exclude warned ruby owners
- reuse bounded completed lines on clean height retries
- Merge pull request #130 from fulgur-rs/perf/c91-3-borrow-edge-input
- Merge pull request #128 from fulgur-rs/fix/font-axis-validation
- Merge pull request #125 from fulgur-rs/perf/c91-1-line-clone
- initialize bounded warnings in iterator controls
- compare internal line ownership across fixed font drivers
- avoid unused previous Line clones in break_all

### Fixed

- Bound owned style data and interning keys before cloning, including first-line
  and ruby inputs, with `Limits::max_style_bytes` (64 MiB by default).

## [0.0.5](https://github.com/fulgur-rs/shodo/compare/v0.0.4...v0.0.5) - 2026-10-01

### Fixed

- *(perf)* bound dense ruby cursor table metadata
- reset intrinsic edge reshape budget per call (shodo-sbp.1)

### Other

- search recent shaping data from the MRU end
- search recent shape plans from the MRU end
- construct preliminary font coordinates only for size adjust
- *(font)* reuse acquired face for run metrics
- *(shape)* construct final run features once
- *(font)* share cached matches for internal shaping
- *(selection)* bypass interval traversal for covered ranges
- *(selection)* prune candidates with a source interval index
- *(ruby)* share cursor rows and retain sparse lane changes
- borrow finalized caret cuts during line index construction
- borrow shaping window scalars and original metadata
- compact ruby candidate column measurement arrays
- restrict ruby candidate annotation lane visits
- *(line)* avoid ancestor vectors in decoration widths
- *(itemize)* finalize following context once per run

### Fixed

- Reset the edge reshape budget for each intrinsic size measurement so reusing a layout context preserves min/max-content widths.

## [0.0.4](https://github.com/fulgur-rs/shodo/compare/v0.0.3...v0.0.4) - 2026-09-30

### Added

- match Parley character-count line breaks
- add processed grapheme limit to line layout
- support CSS word-break manual

### Fixed

- pass first-line cursor map through deep shrink retry
- resolve CSS ch and ic through shaping advance
- *(font)* align ch advance with shaped glyph rounding

### Other

- Merge remote-tracking branch 'origin/main' into feat/aqy-character-limit
- Rescan deeply narrower plain lines before building wide indexes
- Avoid duplicate TCY rejection warnings with first-line styles
- Warn when text-combine-upright: all candidates are rejected across a box boundary
- Exclude upright vertical letters and digits from autospace
- Cache resolved ch and ic units per font layer
- Resolve manual SA text as AL for line breaking
- Merge pull request #87 from fulgur-rs/fix/afq-ch-advance

## [0.0.3](https://github.com/fulgur-rs/shodo/compare/v0.0.2...v0.0.3) - 2026-09-29

### Other

- *(metric_index)* split scalar and content queries
- Merge pull request #83 from fulgur-rs/refactor/c6r56-window-tests
- *(windows)* extract edge-window regression tests
- Merge pull request #81 from fulgur-rs/refactor/c6r53-paragraph-build
- Merge pull request #80 from fulgur-rs/perf/xy2-build-profile
- *(output)* separate line and glyph operations
- *(shape)* extract regression tests
- Merge pull request #76 from fulgur-rs/refactor/c6r54-font-matching-tests
- *(font)* extract matching and retention tests

## [0.0.2](https://github.com/fulgur-rs/shodo/compare/v0.0.1...v0.0.2) - 2026-09-29

### Other

- *(line)* defer edge adjustment to break candidates and walk selectable clusters once
- consolidate Python tools under tools/, reorganize docs/
- split dev/fixtures into fixtures/harness/raikiri crates
- move shodo to crates/shodo, make the root a virtual workspace

## [0.0.1](https://github.com/fulgur-rs/shodo/compare/v0.0.0...v0.0.1) - 2026-09-28

### Added

- *(examples)* render retained ruby annotations
- *(ruby)* expose annotation source and hit relationships
- *(ruby)* align and position ruby with safe overhang
- *(ruby)* lay out coordinated annotation fragments
- *(ruby)* shape annotation lanes and coordinate safe cuts
- *(ruby)* retain paired source-preserving input
- *(examples)* render accepted color emoji bitmaps
- *(fixtures)* add pinned real emoji fonts and opt-in loading
- add optional AccessKit layout adapter
- expose retained accessibility text and positions
- demonstrate retained text colors and solid decorations
- expose source decoration spans in final layout
- retain source paint styles through glyph output
- render and verify vertical layouts
- compose text-combine-upright runs
- preprocess TCY text and select applicable width glyphs
- establish TCY paint geometry and internal carets
- complete vertical shaping and baseline contracts
- shape vertical glyphs with font metrics and transforms
- classify vertical grapheme orientation
- preserve Japanese punctuation boundaries during justification
- integrate punctuation trimming and hanging into line layout
- enforce CSS Japanese line-break strictness
- render fixed real-glyph snapshot cases
- add fixed standalone performance workloads
- add public API glyph and atomic PNG example
- accept resolved first-line styles per inline
- add visual selection and caret navigation
- expose caret geometry and coordinate hit testing
- add bidi-aware inter-script text autospace
- apply consistent character and word spacing to line layout
- resolve horizontal line boxes from actual font metrics
- preserve first-line analysis and shaping resource contracts
- retain first-line text sets and exact continuation cursors
- consume CSS breaks and reshape bounded line edges
- measure and validate bounded line shaping windows
- select intra-cluster breaks and own discretionary glyph sources
- retain shaping instances and complete OpenType style support
- shape matched fonts and retain bounded shaping context
- analyze graphemes and CSS line break opportunities
- apply locale-sensitive text transforms with mappings
- process IFC segment breaks and bidi boundaries
- validate font cache work and decode bounded web font streams
- expose real font metrics units and shared shaper data
- resolve local font faces and ordered CSS sources
- match CSS font ranges and locale-aware cluster fallbacks
- register selected CSS font faces transactionally
- complete bounded line edge reshaping
- plan balanced and pretty line breaks
- measure atomic and float intrinsic widths
- add paragraph line iteration helpers
- support incremental float line layout
- align and justify line fragments
- enforce line block size constraints
- measure inline and atomic line boxes

### Fixed

- *(ruby)* preserve source scopes and bound sequential layout work
- fix accessibility source anchors and hard-break selections
- align mixed vertical tab decorations with text baselines
- align mixed baselines and normalize combined width forms
- preserve combined text spacing and source geometry
- compose hanging advances and respect actual inline fragment edges
- use array slices for fixed RGBA pixels
- validate snapshot ink against the actual canvas
- preserve bidi and break rules for trailing whitespace
- apply preserved whitespace hanging across line layout
- reject incomplete browser boundary captures
- preserve source order before float geometry retries
- justify legal character boundaries and unexpandable lines
- share normalized font selection queries
- bound adversarial shaping and preserve cross-style output
- preserve source markers inside shared shaping clusters
- make fixture regeneration independent of locale
- preserve CSS matching and bounded platform font identities
- preserve cluster geometry and linear line flow
- normalize all line layout inputs

### Other

- *(line)* skip edge-window reshape at unreachable break positions
- Merge main documentation and CI updates into ruby branch
- plan coordinated ruby implementation and verification
- specify source-preserving ruby formatting
- *(emoji)* verify real sequences, boundaries and fallback contracts
- define real emoji fixture and caller rendering plan
- demonstrate accessibility integration and add feature CI
- plan retained accessibility and optional AccessKit integration
- specify accessibility output and optional AccessKit integration
- plan retained paint styles and decoration verification
- specify retained text paint styles and source decoration output
- mark vertical implementation checks complete
- record completed vertical shaping task
- design vertical shaping and composition
- design and plan horizontal Japanese layout
- add fixed glyph snapshots and comparison reports
- plan snapshot rendering and report workflow
- define fixed glyph snapshot harness design
- Record effective Cargo build conditions for performance comparisons
- Add reproducible performance runner and validated baseline artifacts
- Add isolated timing, cold and allocation probes
- pin improved pre-wrap tab boundaries against Chrome
- document Chrome comparison and strict capture updates
- compare raw Chrome breaks and classified differences
- record fixed-font Chromium line boundaries
- add shared seeded browser comparison inputs
- add caller float integration and rollback harness
- define shared glyph paint and source region contracts
- use typed RGBA chunks for current Clippy
- record completed S3 local verification
- explain horizontal layout and hit geometry contracts
- verify horizontal layout and hit resource contracts
- specify horizontal layout and hit testing
- document real shaping and bounded context retention
- specify S2 text analysis and shaping implementation
- document fixture provenance and verify workspace in CI
- expose shared fixtures to development consumers
- add pinned multilingual font and text fixtures
- design reproducible shared development fixtures
- install Fontconfig development files for native builds
- design and plan S1 browser font layer
- verify line flow contracts and document APIs
- cache partial lines during float retries
- plan S0-B line box and flow control
- document current implementation and usage
- Keep hanging spaces already at the paragraph level in their box
- Drop unused Line widths and document glyph run order
- Test block-in-inline results of next_line
- Reuse the enclosing or last interned style before hashing
- Group inline box fragments of reordered lines in one pass
- Apply UAX #9 L1 to hanging spaces and tabs when reordering lines
- Keep inline box ends after bidi controls on the breaking line
- Sanitize style and edge values at build
- Add CI: fmt, clippy, tests, MSRV and wasm32 build
- Order inline box edges with their embedded content
- Derive inline box edge side from its direction
- Reorder line fragments for bidi and split inline boxes into pieces
- Fix inline box continuation at a leading close and bound Glyphs::get
- Add line fragments: glyph runs, inline boxes, atomics and anchors
- Normalize negative inline_start_offset in next_line
- Add greedy next_line with tokens, strut, indent and tab stops
- Build paragraphs: units, bidi levels, required baselines, RichText
- Add placeholder shaper with run-local pen positions
- Fix preserve-spaces handling and saturate DOM offsets
- Collapse white space across elements and build OffsetMapping
- Add ParagraphBuilder with incremental limit checks
- Add FontCollection stub with shared and document layers
- Reject fonts whose layout tables amplify through duplicate references
- Add node identity and style types
- Drop commit attribution lines from the plan
- Add fail-closed Limits and bounded warnings
- Fix unclosed code fence in plan Task 3
- Add writing modes and PhysicalConverter
- Add LayoutUnit fixed-point type
- Ignore local worktrees
- Add S0-A walking skeleton implementation plan
- Add S0 foundation design spec
