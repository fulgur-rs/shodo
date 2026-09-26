# Shared Development Fixtures

## Intent and success

Implement shodo-p2m.10: fixed inputs shared by tests, benchmarks, browser comparisons, and CPU rendering examples. Ordinary shodo library builds must not include these assets or tooling dependencies. Every font and representative case has a stable identifier, reproducible content, selected face index, explicit loading configuration, provenance, and redistribution conditions.

## Structure and alternatives

Use a non-published workspace crate `dev/fixtures` (`shodo-fixtures`). A shared test-only module cannot be imported by separate tools, while assets in the library would increase normal build/data size. The root stays the workspace default member. Native CI tests/lints the entire workspace, including this development crate.

`assets/fonts` contains three renamed, static, face-index-0 subsets of Noto Sans Regular, Noto Sans CJK JP Regular, and Noto Sans Arabic Regular. Download the official upstream files at pinned commits. Preserve all layout features needed for Arabic/contextual shaping and combining marks. Rename derived font names to Shodo Fixture Latin/CJK/Arabic. Store full upstream SIL OFL notices beside the fonts; Rust and original sample prose use the repository MIT OR Apache-2.0 terms.

A JSON manifest records source/commit/URL/input SHA256/output SHA256/byte size/face index/license and subset configuration. A Python FontTools regeneration command uses an exact pinned version, verifies downloads before processing, writes deterministic sfnt output, and compares against checked-in checksums. Updates require an explicit mode; ordinary tests and CI never download/regenerate/change fixtures.

`cases.json` contains original short and long Latin, Japanese, Arabic/RTL, combining-mark and mixed-script samples, family IDs, language, direction, size and width. Include punctuation, spaces, NBSP, soft hyphen, tabs and non-BMP text where supported by the selected corpus; font support tests explicitly distinguish formatting characters from nominal glyphs. No borrowed passage or assumption about Parley asset licenses.

## Public development APIs

- `FontFixture`: stable ID, CSS family, face index, bytes and content SHA256. Three constants, looked up by ID; no filesystem/system enumeration.
- `FixtureCase`: stable ID, text, ordered fixture IDs, lang, direction, font size and line width. Access `cases()` and `case(id)` from the same serialized corpus used for subsetting.
- `load_fonts(&Limits) -> Result<FixtureFonts, FontError>`: constructs `FontCollection::with_options` with system_fonts=false, registers the three descriptor faces once, and keeps IDs/configuration available. Configures deterministic generic/fallback lists.
- `FixtureCase::build(&mut LayoutContext, &FixtureFonts, &Limits) -> Result<Paragraph, LimitExceeded>`: constructs the paragraph through the public builder, preserving selected families/language/direction.

Consumers are an integration test using actual cmap/metrics/shaper data and two small development examples: inspect font metadata and lay out corpus cases through public APIs. S1 has real fonts/metrics; paragraph shaping remains the documented S0 stub until S2. Do not treat codepoints emitted by that stub as real glyph IDs or assert equal behavior to a real browser.

## Verification

Offline normal tests validate IDs/configuration, checksums, licenses/provenance, nominal coverage of every required character, and successful font acceptance/shaper construction. Actual shaping verifies Arabic layout tables survived the subset, independently of the S0 paragraph path. Samples use the same loading API in at least two consumers. Source downloads and reproducible regeneration are a separate maintainer operation. Stable/MSRV1.89, no-default library, wasm, fmt, Clippy and rustdoc retain their checks. Bound total checked-in font data to 512KiB; if exceeded, tighten subsets without dropping sample coverage or layout behavior.

## Review risks

- Subsetting Arabic must retain contextual GSUB/GPOS.
- Manifest, bytes, font metadata, and case configuration must agree.
- Mixed text must be covered by the configured family chain without OS fonts.
- Generation cannot silently accept a replaced upstream download or mutate baselines in check mode.
- Development dependencies/assets must not become regular library dependencies.
