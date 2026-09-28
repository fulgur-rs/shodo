# Real emoji fixtures, sequence contracts and bitmap caller integration

Issue: `shodo-7zg`. This is architectural work spanning fixed assets, font
selection/shaping verification and a caller rasterization path. The user's
autonomous claim/worktree/implementation/PR/CI-success merge instruction applies;
design and plan receive explicit author self-review, not purported human approval.
The raikiri S4 spike remains untouched and unmerged; `shodo-p2m.6` is excluded.

## Outcome and approach

Validate real fonts for single emoji, VS15/VS16, skin tones, ZWJ sequences,
regional-indicator flags, keycaps and tag sequences. Every original byte must
retain source mapping; a compound grapheme stays indivisible for ordinary and
emergency breaks, caret navigation, hit testing and selection. Mixed Japanese
and Latin text retains accepted widths, baselines and line heights. Demonstrate
color pixels drawn from the public accepted glyph/font output, without reshaping.
Document font absence, sequence limitations and caller-owned color formats.

Three viable caller options are a CBDT/CBLC PNG decoder, a COLRv1 paint-graph
renderer, or both. The first uses the existing skrifa/png/tiny-skia dependencies
and a fixed bitmap strike; COLRv1 adds palette/gradient/composite traversal; both
expand the validation matrix. Select CBDT/CBLC PNG for the actual integration
example and keep the core's font-byte/glyph handoff independent of format.
Retain the existing outline path for monochrome and surrounding text.

## Fixed real fonts and assets

Use the official Noto Color Emoji Unicode13.1 release at
`aac7ccaa4d1dea4543453b96f7d6fc47066a57ff`, file `fonts/NotoColorEmoji.ttf`:
source10419948bytes, SHA256
`2b106b88034043001f28b3ee8e58f62e867f0f0666ea072a4acf6cbd956ca160`.
It is an actual CBDT/CBLC font with a109ppem strike and no glyf/CFF outline.
The latest upstream source-tree revision lacks checked-in fonts and has no
gh-pages branch; the pinned older release contains every requested representative.
This fixture does not establish support for all newer Unicode emoji.

Use the official Google Fonts Noto Emoji variable monochrome face at
`23e54b51ddffbc7713c583748e3bd86f62b1fa4a`,
`ofl/notoemoji/NotoEmoji[wght].ttf`: source1982596bytes, SHA256
`de6c18832938afc99caf132b39d6a30a19bac7f2e812e28db2535b4608d27551`.
It has glyf/gvar outlines and a wght axis. Copy the corresponding OFL files,
retain copyright/license names and rename the subsets to Shodo Fixture Emoji.
The mono axis is wght300..700, default400; its named instances have no explicit
PostScript name IDs. Preserve these instances and retain their weight styles.
Both faces share this family for existing same-family presentation selection.

Add `assets/fonts/emoji-color.ttf`, `assets/fonts/emoji-mono.ttf`, source notices,
license files and manifest entries with exact source/subset checksums and sizes.
`assets/emoji-cases.json` is a separate original corpus. It includes `😀`,
`☺︎`, `☺️`, `👍🏽`, `👩‍💻`, `👨‍👩‍👧‍👦`, `🇯🇵`, `1️⃣`, the Scotland flag
(`U+1F3F4 E0067 E0062 E0073 E0063 E0074 E007F`) and mixed Latin/Japanese text.

Each manifest font may specify a `corpus` path, defaulting to
`assets/cases.json`; only the two emoji entries select `assets/emoji-cases.json`.
Thus regeneration does not silently add emoji symbols to the existing three
subsets. Reproduce all five from their pinned original bytes with FontTools4.61.1
and assert the original three files/checksums stay unchanged. Subset all layout
features and required GSUB closure; do not discard components/alternatives to
fit a size target. Preflight used the required sequences and measured160316byte
color/26772byte mono subsets,591552bytes including existing assets. Family rename
changes the final checksums/sizes. Raise the total font budget to768KiB, keeping
the staging-before-update and source-checksum rejection contracts. Enforce
the total budget in offline checking as well as rebuild/update; an over-budget
update must leave every checked-in font and the manifest untouched.

## Development fixture interface

Keep `FONTS`, `load_fonts`, its three IDs and `cases()` unchanged. Extend `font(id)`
to search the extra fixtures and add:

- `EMOJI_FONTS: &[FontFixture]`, in color then mono order, IDs `emoji-color` and
  `emoji-mono`, both family `Shodo Fixture Emoji`.
- `EmojiFixtureFonts { base: FixtureFonts, emoji_ids: [FontId; 2] }`.
- `load_emoji_fonts(&Limits) -> Result<EmojiFixtureFonts, FontError>`, loading the
  existing faces first, then the two emoji faces with system discovery disabled.
  Extend deterministic generic/fallback registration for the shared emoji family.
- `emoji_cases() -> &'static [FixtureCase]`, parsing the separate corpus once.

These are development-crate interfaces, not new dependencies or assets in shodo.
Ordinary fixture consumers keep their previous font set and snapshot results.

## Selection, sequence support and missing fonts

Use the existing `FontPresentation::{Auto,Text,Emoji}` and CSS family/style search.
Auto prefers the color/mono face as directed by VS16/VS15 or Emoji_Presentation;
explicit presentation is verified too. Real matching is scoped to the entire
grapheme; none of its components may pick a separate face. The matched family
and instance must agree with the accepted glyphs and retained bytes.

Nominal cmap coverage establishes eligibility, not a guarantee of GSUB sequence
composition. Preserve the declared separation: match the grapheme, then shape
the selected face. Do not infer sequence capability solely from glyph count:
legitimate font representations can have multiple glyphs. A nominally covered
but uncomposed sequence can produce visible components from the selected face;
keep it one selectable/breakable grapheme and document that the matcher does not
rescan eligible faces based on a one-glyph heuristic. Verify this with a derived
real-font test copy whose GSUB is removed, and with an unsupported ZWJ sequence.
This is a limitation contract, not a full CSS/browser or Unicode-RGI claim.

Missing nominal coverage falls back as a whole grapheme through configured
families, without system discovery in the fixture. If no face covers it, retain
the current `WarningKind::Unsupported` missing-font warning and glyph ID0
(`.notdef`) alternative output, with complete source coverage. Its font bytes
come from the existing primary-face fallback; do not claim a missing emoji
picture was rendered successfully.
Tests use old `load_fonts` and an unrepresented emoji to demonstrate this. Do
not fabricate a composed picture or count a fallback as sequence support.
Any real-font test revealing a correctness defect in selection, shaping, breaks,
metrics or source positions gets a reproducing runtime test and a core fix.

## Caller bitmap renderer

Extend the existing shared `examples/support/glyph_paint.rs` path, factoring
bitmap decoding/placement into `examples/support/bitmap_paint.rs`. Existing
entrypoints keep their signatures. Before outline drawing, try the actual
accepted glyph ID in `BitmapFormat::Cbdt`; draw a supported PNG bitmap once.
Other text continues through the existing outline code. Font bytes, face index,
font size, normalized coordinates, glyph origins and logical-to-physical matrix
come from `GlyphRunView` and its retained `font_data()`. Source strings are not
inputs to the bitmap draw function and no new layout/shaping is performed.

Decode PNG into premultiplied RGBA, preserve intrinsic colors, and use the
strike's ppem and bearings to place its image relative to the accepted origin.
Apply the public glyph matrix and each line's PhysicalConverter. Core advances
and baselines remain authoritative; bitmap advance/size does not reposition the
next glyph or redefine line metrics. Support horizontal and upright vertical
placement through these existing transforms. Reject invalid dimensions,
nonfinite placement, unsupported bitmap data and synthesis, malformed images,
and a transformed image outside a declared canvas. Validate decoded dimensions
against bitmap metrics and bound output before allocating a pixel buffer.
Publish clear `PaintError` variants for invalid/unsupported bitmap data.

COLR/CPAL v0/v1, sbix and SVG remain accessible as retained font bytes for caller
backends; the sample's executable support is CBDT/CBLC PNG plus outlines.
EBDT masks, other CBDT encodings, unsupported color graphs/SVG and font synthesis
are not claimed by this sample. Generic core table detection is not a rasterizer
capability guarantee. Explain these distinctions in `docs/emoji.md`.

Add `examples/emoji_png.rs`: fixed fonts, actual mixed/script sequences, accepted
glyphs/positions, caller bitmap output and an explicitly chosen PNG output path.
Require actual colored pixels and identify the selected font/glyphs. Include a
runtime test proving color output from the real font and placement from supplied
glyph IDs/origins, without a source-string fallback.

## Verification and completion

Named contracts to cover: pinned real color/mono tables/licenses/reproduction;
presentation and selected-instance identity; each requested sequence shaping;
cmap-without-GSUB limitation; missing-font whole-grapheme fallback; narrow widths
and emergency/normal breaks; accepted caret/hit/selection boundaries; split DOM
emoji source mapping; mixed Japanese/Latin metrics; actual color pixels and
bitmap bearings/transforms; invalid image/canvas/synthesis failure.
Derive literal glyph/metric expectations from independently inspected font tables
and bitmaps, not from the shodo functions under test. Preserve existing snapshots.

The full baseline on merge2081865 is684passed/0failed across53binaries. Before
integration run stable/MSRV workspace suites, AccessKit-enabled suites, default/
no-default/complex/all-feature modes, root wasm modes, fmt/Clippy/rustdoc warnings
denied, allocator/benchmark, fixed snapshot matrix, Python/asset/reproduction and
the actual emoji example. Add CI execution of the new example. Use one fresh
whole-branch review and one Critical/Important RED→GREEN fix pass; defer Minors.
Then push/create PR, verify exact-head check/msrv/wasm success, merge, close the
issue and archive/report decisions before owned cleanup and the next ready issue.

Primary references inspected2026-09-28: [Noto Emoji](https://github.com/googlefonts/noto-emoji),
[pinned color release](https://github.com/googlefonts/noto-emoji/tree/aac7ccaa4d1dea4543453b96f7d6fc47066a57ff/fonts),
[pinned mono source](https://github.com/google/fonts/tree/23e54b51ddffbc7713c583748e3bd86f62b1fa4a/ofl/notoemoji),
[CSS Fonts4 cluster matching](https://www.w3.org/TR/css-fonts-4/#cluster-matching),
and the cached skrifa0.44.0 bitmap and existing accepted-glyph APIs.
