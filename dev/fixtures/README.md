# Shared development fixtures

`shodo-fixtures` is an unpublished workspace crate for tests, benchmarks, browser
recorders and drawing tools. It owns fixed fonts and original prose; shodo's normal
dependency graph does not include it. The root library remains the default workspace
member. All consumers load the same face index, font bytes and case settings with
system font discovery disabled.

## Use

From the repository root:

```sh
cargo test --workspace
cargo run -p shodo-fixtures --example inspect_fonts
cargo run -p shodo-fixtures --example layout_cases
cargo run -p shodo-fixtures --example float_png -- target/shodo-floats.png
cargo run -p shodo-fixtures --example layout_cases -- arabic-short
python3 dev/fixtures/tools/regenerate.py --check
```

A separate development tool can add `shodo-fixtures = { path = "../fixtures" }`
(adjust the path) and use `load_fonts(&Limits)`, `cases()` or `case("mixed-scripts")`.
`FixtureCase::build` uses public shodo APIs and the case's ordered font IDs, language,
direction and size; line layout uses the case's width. `font(id)` exposes immutable
bytes/face index/identity for renderers or browser font embedding. `FixtureFonts::ids`
are runtime FontIds for that collection, ordered like `FONTS`; use stable string
IDs in snapshots/recorded data, not those runtime IDs.

The font-information example reads real metrics and shaping data. The paragraph
layout example shapes with the fixed fonts through shodo and reports real glyph
counts. Corpus tests check actual face IDs, contextual Arabic shaping, Latin
ligatures/kerning, source ranges and finite positions. These results are not
Chrome reference baselines; a browser comparison needs its own recorder.

`cargo run -p shodo-fixtures --example shape_timing` measures paragraph builds for
all 12 cases before and after context reuse and `shrink_to(0)`, excluding font
registration. See [the measurement record](../../docs/shaping-measurements.md).

## Data and provenance

| ID | Derived family | Format / index | Bytes | Upstream source |
| --- | --- | --- | ---: | --- |
| latin | Shodo Fixture Latin | TrueType / 0 | 97,232 | Noto Sans Regular |
| cjk | Shodo Fixture CJK | CFF OpenType / 0 | 210,332 | Noto Sans CJK JP Regular |
| arabic | Shodo Fixture Arabic | TrueType / 0 | 96,900 | Noto Sans Arabic Regular |

Total font data: 404,464 bytes (about 395 KiB). The originals total 17,277,400 bytes;
subsets retain the corpus, useful ASCII/Latin/combining, kana/CJK punctuation/full-width,
and Arabic ranges rather than shipping that entire set. Other characters are not
promised. Every visible corpus character is tested against its ordered font chain.
Formatting LF, TAB and soft hyphen have separate layout semantics. Arabic contextual
forms and mark positioning are exercised through actual shaping. The Japanese
supplementary-plane case includes U+20BB7 for UTF-16/UTF-8 conversion tests.

[assets/manifest.json](assets/manifest.json) is the authoritative provenance record:
immutable upstream commit URLs, original SHA256, generated SHA256, size, face index,
license location and extra subset ranges. The sources are the official Noto
repositories: [noto-fonts](https://github.com/notofonts/noto-fonts/tree/ffebf8c1ee449e544955a7e813c54f9b73848eac)
and [noto-cjk](https://github.com/notofonts/noto-cjk/tree/f8d157532fbfaeda587e826d4cd5b21a49186f7c).
The source sizes are 569,208 / 16,467,736 / 240,456 bytes respectively.

All three font subsets remain **SIL Open Font License 1.1**, separately from the
crate's code license. Complete upstream OFL notices and extracted original font
copyright notices are in [assets/licenses](assets/licenses). Derived family/full/
PostScript names, including CFF internal names, are changed to Shodo Fixture names;
original copyright/license name records remain. Retain these notices whenever
redistributing the font bytes. Do not advertise author endorsement or sell the font
files alone; consult the included OFL conditions for redistribution.

[assets/cases.json](assets/cases.json) contains 12 original sample cases written for
this repository (MIT OR Apache-2.0, like the code), not passages copied from Parley
or other works. Stable IDs distinguish short/long Latin, Japanese, Arabic/RTL,
whitespace/soft hyphen, language-sensitive casing, combining marks and mixed scripts.
Each record fixes text, ordered font IDs, language, direction, font size and width.
Cases are parsed once and shared by all consumers and the subset generator.

## Reproduce and update

Normal tests and `--check` run offline; they never modify expected data. Maintainer
regeneration needs Python and the exact FontTools version in tools/requirements.txt:

```sh
python3 -m venv /tmp/shodo-fonttools
/tmp/shodo-fonttools/bin/python -m pip install -r dev/fixtures/tools/requirements.txt
/tmp/shodo-fonttools/bin/python -m unittest discover -s dev/fixtures/tools -v
/tmp/shodo-fonttools/bin/python dev/fixtures/tools/regenerate.py --rebuild
```

`--rebuild` downloads only the pinned sources, verifies each original checksum,
reproduces the subsets with all layout features and renamed metadata, then requires
the generated checksums to match. It never replaces checked-in fonts or the manifest.
To avoid downloads, pass `--sources-dir /path/to/originals` with `latin.ttf`, `cjk.otf`
`arabic.ttf`, `emoji-color.ttf` and `emoji-mono.ttf` downloaded from the manifest URLs. Inputs must still match the
recorded hashes. Subset ordering is canonical and original timestamps are preserved.

For an intentional corpus/coverage/font update:

1. Edit the corpus and/or subset ranges; when changing upstream, update immutable
   URLs, original checksum and license/copyright notices from the new release.
2. Run the same command with **`--update`**. All inputs and outputs are staged and
   validated before checked-in assets are replaced; the 768KiB total font budget applies to both offline checking and rebuild/update.
3. Copy updated family/face/hash values from the manifest into `FONTS` in src/lib.rs;
   update this size table if needed. Tests catch any disagreement.
4. Run `--check`, `--rebuild`, Python tests, `cargo test --workspace`, and both
   consumer examples. Review coverage, Arabic shaping, provenance and byte changes.
5. Review/commit the intended diff. Changes to browser data or approved images in
   later tools require their own explicit baseline-update commands.

CI checks the workspace and offline asset integrity/names, and tests Rust 1.89. It
never downloads font sources, regenerates fonts or updates expected outputs. FontTools
is development-only; ordinary cargo users do not need Python. The separate
`--rebuild` maintainer check verifies reproducibility against the original sources.

The CJK subset also retains 水 (U+6C34) for size-adjust metric verification.

The [float caller harness](../../docs/float-integration-harness.md) shares an owned
Taffy checkpoint driver between numeric regressions and the fixed-font PNG example.
It remains dev-only; root shodo consumers acquire no Taffy dependency.

The [Chrome comparison](../../docs/browser-comparison.md) shares seeded fixed inputs
through `shodo_fixtures::browser`, checks saved source positions/boundary widths
offline, and reports every raw difference. Recollection is an explicit Python
standard-library/headless Chromium command using a disposable profile.

## Optional real emoji fixtures

`load_emoji_fonts(&limits)` loads the original three faces, then `EMOJI_FONTS`
(color, mono) into `EmojiFixtureFonts::base.collection`. `emoji_ids` identifies
those two faces. The original `load_fonts`, `FONTS`, `cases()` and their registration
order remain unchanged. Both emoji faces use the derived family **Shodo Fixture
Emoji**, enabling VS15/VS16 and explicit presentation selection within that family.
`font(id)` also resolves these optional assets. `emoji_cases()` is a separate
original corpus containing the representative sequences and mixed Japanese/Latin.

| ID | Format / index | Bytes | Original source |
| --- | --- | ---: | --- |
| emoji-color | CBDT/CBLC PNG, 109ppem / 0 | 160,272 | Noto Color Emoji Unicode13.1 |
| emoji-mono | Variable TrueType, wght300..700 / 0 | 26,736 | Noto Emoji |

All five subsets total **591,472 bytes**, within the768KiB budget. The original
three remain byte-identical. These two are separately licensed under OFL1.1;
complete pinned notices and original copyright/license records are in
[assets/licenses](assets/licenses). Sources and SHA256 values are recorded in
[assets/manifest.json](assets/manifest.json):
[color source](https://github.com/googlefonts/noto-emoji/tree/aac7ccaa4d1dea4543453b96f7d6fc47066a57ff/fonts)
and [mono source](https://github.com/google/fonts/tree/23e54b51ddffbc7713c583748e3bd86f62b1fa4a/ofl/notoemoji).
The fixture tests use these actual font bytes; a color-table marker is insufficient.

Only the two emoji manifest entries set `corpus` to `assets/emoji-cases.json`.
Other entries default to `assets/cases.json`, so adding the optional corpus does
not extend the old subsets. Reproduction keeps all layout features and substitution
closure. It verifies every source before updating assets; source mismatch or total
budget overflow leaves checked-in outputs untouched. Subsets are a fixed test
corpus, not coverage of every Unicode emoji or host fallback policy.

See [emoji layout and caller color drawing](../../docs/emoji.md) for sequence
limitations, missing-font behavior and `cargo run -p shodo-fixtures --example
emoji_png -- OUTPUT.png`. The renderer draws accepted CBDT PNG glyphs and outlines.

## Retained ruby output

Run `cargo run -p shodo-fixtures --example ruby_png -- target/ruby-png`. The
example writes horizontal, VerticalRl and VerticalLr PNG/JSON pairs, using only
the registered CJK fixture. JSON keeps base and annotation source ranges separate
and records retained glyph IDs, font checksums, sizes, advances, origins, transforms
and overflow. The shared painter traverses visible lanes and composes their
transforms before physical conversion; it reuses outline/bitmap and source
decoration paths. `ruby_paint` pins source colors, asymmetric vertical outlines,
nested translations, hidden/ collapsed lanes and strict clipping. See
[the caller contract](../../docs/ruby.md). The existing browser recorder does not
measure ruby and its fixture exclusions remain applicable.

The [raikiri style diagnostic](../../docs/raikiri-style-diagnostics.md) replays original
WPT HTML/CSS with the real pinned parser/cascade, verifies original resource
bytes, and identifies residual fields from the frozen S4 caller profile. It
keeps the integration spike separate and emits no WPT image verdict.
