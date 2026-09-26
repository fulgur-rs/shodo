# Shared Development Fixtures Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Complete shodo-p2m.10 with reproducible fixed fonts, text and case configuration shared by development consumers.
**Architecture:** A non-published dev/fixtures workspace crate owns assets, a manifest/corpus, loading APIs and examples. Root shodo remains the default member and normal library dependencies stay free of fixture assets.
**Tech Stack:** Rust1.89, shodo public font/layout APIs, serde_json, SHA256, Python FontTools pinned for maintainer-only regeneration.
**Spec:** docs/superpowers/specs/2026-09-27-shodo-dev-fixtures-design.md

## Global Constraints

- No OS font dependency or normal-test network access; root default member remains shodo.
- Three renamed static Noto subsets, face index0; full OFL notices and pinned source/input/output checksums.
- Corpus prose is original MIT OR Apache-2.0; do not assume Parley assets share its code license.
- Retain Arabic contextual shaping/combining coverage; total font files <=512KiB.
- S0 paragraph stub is documented; only direct real shaping tests may use actual glyph IDs until S2.
- Stable/MSRV1.89, no-default library and wasm remain supported.

## Review Focus

- A substituted/truncated download must fail before regeneration writes anything (Task1).
- Check mode must leave all checked-in files untouched even when a checksum differs (Task1).
- RTL shaping and combining glyph positions must survive subset closure (Task2).
- Mixed-script family chains must cover all visible sample characters (Task2).
- Ordinary shodo dependency graph/build must not include fixture bytes/tools (Task3).

### Task 1: pinned source corpus and reproducible subset assets

**Files:** dev/fixtures/assets/cases.json, assets/manifest.json, assets/fonts/*, assets/licenses/*, tools/regenerate.py, tools/test_regenerate.py.
**Interfaces:** manifest format_version1, fonts entries with id/family/path/face_index/source_url/source_sha256/sha256/size/license; cases entries id/text/font_ids/lang/direction/font_size/width.
- [x] Write Python tests for artifact completeness, manifest source SHA256 mismatch rejection and check-mode immutability; run unittest and observe missing assets/API RED.
- [x] Add original Latin/Japanese/Arabic/combining/mixed corpus; download pinned Noto official sources/license, subset with FontTools exact version, rename font names, preserve layout features; save manifest and checksums.
- [x] Implement `regenerate.py --check` (verify stored assets without network), `--rebuild` (pinned verified input, compare outputs without modifying repo), `--update` (explicit replacement). All build outputs stage outside assets until every source/output passes validation.
- [x] Run unittest and check/rebuild, compare output bytes, full root suite; commit.

### Task 2: shared development crate and real consumers

**Files:** Cargo.toml, dev/fixtures/Cargo.toml, src/lib.rs, tests/fixtures.rs, examples/inspect_fonts.rs, examples/layout_cases.rs.
**Interfaces:** FontFixture(id,family,bytes,face_index,sha256); FixtureCase(id,text,font_ids,lang,direction,font_size,width); cases(), case(&str), font(&str), load_fonts(&Limits)->Result<FixtureFonts,FontError>, FixtureCase::build(&mut LayoutContext,&FixtureFonts,&Limits)->Result<Paragraph,LimitExceeded>.
- [x] Add integration tests for unique IDs, fixed checksums/index, all-case nominal glyph coverage through family chains, accepted font metrics and shaper data, ordered case lookup, offline public paragraph build/line layout, and Arabic GSUB contextual substitution. Run workspace fixture tests and observe missing API RED.
- [x] Implement public development APIs and original case loading; disable OS fonts, set fixed generic/locale fallbacks, keep default library dependency graph separate.
- [x] Add independent font-inspection and public case-layout examples; execute both and verify shared IDs/settings. Explicitly print paragraph stub limitation.
- [x] Run full workspace suite and clippy, commit.

### Task 3: documentation, CI and final integration

**Files:** dev/fixtures/README.md, root README.md, .github/workflows/ci.yml, plan status.
- [ ] Document license/source/size/coverage/update procedure, both consumers, fixture API, and stub limitations.
- [ ] Test/lint workspace in native CI while keeping root default member and wasm library build; validate regular dependency graph excludes shodo-fixtures.
- [ ] Run fmt, workspace stable/MSRV tests, no-default root tests, wasm, rustdoc, Python asset checks and deterministic rebuild; commit.
- [ ] Fresh whole-branch review, one Important/Critical RED→GREEN fix pass, PR/CI-success merge, close issue and remove worktree under user's existing authorization.
