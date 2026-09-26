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

The font-information example reads real S1 metrics and shaping data. The paragraph
layout example still uses the S0 stub until S2 integrates real shaping: its output
labels this limitation. These line counts are not real-font Chrome baselines, and
stub codepoints must not be interpreted as these fonts' glyph IDs. The acceptance
test directly shapes Arabic with harfrust to check retained GSUB/GPOS without
pretending that the paragraph pipeline already does so.

## Data and provenance

| ID | Derived family | Format / index | Bytes | Upstream source |
| --- | --- | --- | ---: | --- |
| latin | Shodo Fixture Latin | TrueType / 0 | 97,232 | Noto Sans Regular |
| cjk | Shodo Fixture CJK | CFF OpenType / 0 | 209,988 | Noto Sans CJK JP Regular |
| arabic | Shodo Fixture Arabic | TrueType / 0 | 96,900 | Noto Sans Arabic Regular |

Total font data: 404,120 bytes (about 395 KiB). The originals total 17,277,400 bytes;
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
and `arabic.ttf` downloaded from the manifest URLs. Inputs must still match the
recorded hashes. Subset ordering is canonical and original timestamps are preserved.

For an intentional corpus/coverage/font update:

1. Edit the corpus and/or subset ranges; when changing upstream, update immutable
   URLs, original checksum and license/copyright notices from the new release.
2. Run the same command with **`--update`**. All inputs and outputs are staged and
   validated before checked-in assets are replaced; the 512KiB font budget applies.
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
