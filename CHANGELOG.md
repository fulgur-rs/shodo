# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
