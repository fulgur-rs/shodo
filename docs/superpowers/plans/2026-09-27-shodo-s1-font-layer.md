# S1 Font Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Complete shodo-p2m.2 with document-local CSS font matching and real font metrics.

**Architecture:** Extend the existing layer state with fontique metadata, CSS descriptors and bounded caches. Keep S0 stub behavior until S2 replaces paragraph shaping.

**Tech Stack:** fontique 0.11, skrifa 0.44, harfrust 0.12, optional wuff 0.2; safe Rust, MSRV 1.89.

**Spec:** docs/superpowers/specs/2026-09-27-shodo-s1-font-layer-design.md

## Global Constraints

- Keep document layers unshared, Send+Sync and isolated; retain generations and stable IDs.
- Default system-fonts is lazy and runtime-disableable; wasm must build.
- Do not change S0 paragraph shaping or fixed-width tests in this issue.
- Preserve resource limits, malformed-input errors and no partial registration.
- Tests use synthetic repository-owned fonts and no platform installed fonts.

## Review Focus

- Family order must outrank document priority between different family names (Task 2).
- Late font registration must invalidate cached misses in child layers (Task 2).
- A local font's full name is not necessarily its family (Task 3).
- Declared decompressed lengths can be false (Task 5).
- A zero-entry cache still must return usable independent handles (Task 4).

### Task 1: descriptors and transactional registration

**Files:** src/font/mod.rs, src/font/descriptor.rs, src/font/tests.rs, Cargo.toml.
**Interfaces:** FontFaceDescriptor { family, weight: (f32,f32), width: (f32,f32), style, unicode_ranges: Vec<(u32,u32)> }; register_face(data: Vec<u8>, index: u32, descriptor: FontFaceDescriptor) -> Result<FontId,FontError>.
- [ ] Add synthetic sfnt helper with real head/hhea/maxp/hmtx/cmap/name data and descriptor registration tests: selected TTC index, invalid ranges and budgets leave generation unchanged.
- [ ] Run `cargo test font::` and confirm new APIs are missing (RED).
- [ ] Implement descriptor validation, selected face storage, fontique metadata registration; preserve register and stub APIs.
- [ ] Run font tests and complete suite (GREEN); commit.

### Task 2: CSS matcher and fallback configuration

**Files:** src/font/matching.rs, src/font/mod.rs, src/font/tests.rs.
**Interfaces:** FontQuery, FontMatch, FontCollection::match_cluster(&FontQuery,&str)->Option<FontMatch>; set_generic_families(GenericFamily,Vec<String>); set_fallback_families(script:[u8;4], language:Option<String>,families:Vec<String>).
- [ ] Add tests for CSS family/weight/width/style priority, unicode-range, whole cluster, locale CJK, emoji presentation, missing match and generations invalidating cached misses.
- [ ] Run `cargo test font::` (RED).
- [ ] Implement CSS ranking on descriptors and fontique native family candidates, lazy system enumeration and configurable generics/fallbacks; use bounded cache of query+cluster+generations.
- [ ] Run font tests and complete suite (GREEN); commit.

### Task 3: local() and ordered source registration

**Files:** src/font/source.rs, src/font/mod.rs, src/font/tests.rs.
**Interfaces:** FontSource::{Local(String),Data(Vec<u8>,u32)}; register_sources(descriptor:FontFaceDescriptor,sources:Vec<FontSource>)->Result<FontId,FontError>.
- [ ] Test PostScript/full-name local resolution, source order, unavailable local fallback, missing source errors and document isolation (RED).
- [ ] Resolve trusted installed/bundled faces by full/PostScript names; alias selected face with CSS descriptor without family rewriting (GREEN).
- [ ] Run full suite; commit.

### Task 4: metrics, units, shaper cache and lifecycle

**Files:** src/font/metrics.rs, src/font/mod.rs, src/font/tests.rs.
**Interfaces:** metrics_with_coords(id,size,coords)->Option<FontMetrics>; vertical_metrics(id,size,coords); resolve_ch/resolve_ic(&FontQuery,size)->FontUnit; shaper_data(id)->Option<Arc<harfrust::ShaperData>>; layer_handle()->WeakFontLayer.
- [ ] Test actual ascent/descent/linegap/decoration, glyph widths and missing-unit defaults, cache sharing/eviction/zero cap, dropped-layer notification and Send+Sync (RED).
- [ ] Implement skrifa metrics and shared bounded LRU; check layer allocation without ID reuse (GREEN).
- [ ] Run full suite; commit.

### Task 5: font structure and bounded web-font decoding

**Files:** src/font/check.rs, src/font/source.rs, src/limits.rs, src/font/tests.rs, Cargo.toml.
**Interfaces:** decode_web_font(&[u8],&Limits)->Result<Vec<u8>,FontError> plus ordered-source integration.
- [ ] Test repeated Coverage/ClassDef/GDEF/AAT references, bad signatures, malformed WOFF/WOFF2, valid compressed font and expansion beyond declared size (RED).
- [ ] Add bounded validation and decompression callbacks; enforce limits before retaining output (GREEN).
- [ ] Run full suite and no-default-features; commit.

### Task 6: documentation and final verification

**Files:** README.md, public font API rustdoc, plan progress.
- [ ] Add an executable doc example demonstrating document CSS registration and font-unit resolution.
- [ ] Run fmt, clippy -D warnings, stable and 1.89 test suites, no-default-features tests, wasm build and rustdoc -D warnings.
- [ ] Review complete diff against spec and issue; fix important defects with RED→GREEN regressions.
- [ ] Create PR against main, verify CI for exact head, merge only after success, close issue and remove worktree.
