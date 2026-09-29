# shodo

A Rust text typesetting library that lays out the inline content of a single
block container: a paragraph. Built for [raikiri](https://github.com/fulgur-rs/raikiri)
as a replacement for Parley, shodo can also be used with styled text without a DOM.

**Early development:** public APIs may change. The repository's design documents
include planned work as well as implemented behavior; use the guides below and
the source API documentation for current contracts.

## What it does

- Styled text, nested inline boxes, atomic inlines such as images, and forced breaks.
- CSS whitespace and text transforms, Unicode boundaries, bidirectional text, and
  whole-grapheme font fallback with harfrust OpenType shaping.
- Incremental line breaking, alignment, justification, tabs, indentation,
  `::first-line` styles, intrinsic widths, and `balance` / `pretty` break plans.
- Japanese line-break restrictions, punctuation spacing and hanging; vertical and
  sideways writing, glyph orientation, and `text-combine-upright: all`.
- Ruby pairing, coordinated wrapping, alignment, placement and safe overhang
  in horizontal and vertical text. See the [ruby contract](docs/ruby.md).
- Retained glyph/font output, solid text paint and decoration geometry, source
  mapping, caret placement, hit testing, selections, and accessibility text.
- Shared and document-local font collections, system fonts, WOFF/WOFF2 decoding,
  resource limits, and diagnostic warnings.

The caller supplies computed styles, sizes and baselines for atomic inlines,
places floats, and renders the accepted fragments. shodo does not resolve CSS,
lay out an entire page, or rasterize glyphs.

## Getting started

Requires **Rust 1.89.0 or later** (edition 2024). On Linux, the default
`system-fonts` feature also requires Fontconfig development files and pkg-config
(on Debian/Ubuntu, `libfontconfig1-dev` and `pkg-config`).

```sh
git clone https://github.com/fulgur-rs/shodo.git
cd shodo
cargo build
cargo run --example horizontal_hit
```

`horizontal_hit` prints layout and hit-test geometry. Without a font path, it uses
deterministic missing-font fallback; pass `-- path/to/a-font.ttf` to use a real face.

To use the published crate from another project:

```toml
[dependencies]
shodo = "0.0.1"
```

To use a local checkout instead, add a path dependency, adjusting the path to
your checkout:

```toml
[dependencies]
shodo = { path = "../shodo/crates/shodo" }
```

Build styled text and lay it out at a fixed width:

```rust
use shodo::font::FontCollection;
use shodo::limits::Limits;
use shodo::style::{InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, RichText};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let limits = Limits::default();
    let fonts = FontCollection::new(&limits);
    let mut cx = LayoutContext::new();
    let style = InlineStyle {
        font_size: 16.0,
        ..InlineStyle::default()
    };
    let paragraph_style = ParagraphStyle {
        root: style.clone(),
        ..ParagraphStyle::default()
    };
    let paragraph = RichText::with_limits(&paragraph_style, &limits)
        .push("Hello world from shodo", &style)
        .build(&mut cx, &fonts)?;

    let lines = paragraph.break_all(
        &mut cx,
        &LineOptions::default(),
        160.0,
        &AtomicSizes::EMPTY,
    );
    for line in &lines {
        println!("{}", &line.text()[line.text_range()]);
    }
    Ok(())
}
```

This example uses system font discovery. Results depend on installed fonts; for
deterministic output, disable discovery and register bundled font data. Without
a matching font, shodo emits warning-backed missing-glyph output, which is not a
drawable substitute for a font. Read warnings through `Paragraph::warnings()`
and `LayoutContext::take_warnings()`.

`break_all` is for fixed-width layout without float placement. Use `next_line`
for changing widths, page-height constraints, or incremental float integration.
The [integration guide](docs/integration.md) covers that loop, font registration,
output ownership, resource budgets, and coordinate conventions.

## Cargo features

| Feature | Default | Purpose |
| --- | --- | --- |
| `system-fonts` | Yes | Discover system fonts on supported native platforms. |
| `web-fonts` | Yes | Decode WOFF/WOFF2; the caller fetches URLs. |
| `complex-scripts` | Yes | ICU dictionary/neural segmentation for complex-context scripts. |
| `accesskit` | No | Adapt retained accessibility output to AccessKit. |

For bundled sfnt/TTC fonts without system discovery or web-font decoding:

```toml
[dependencies]
shodo = { path = "../shodo/crates/shodo", default-features = false, features = ["complex-scripts"] }
```

Font matching, metrics, and OpenType shaping remain available with all default
features disabled. Without `complex-scripts`, complex-context scripts use
non-dictionary segmentation and report the degradation; Arabic shaping remains
available. Wasm builds use memory-backed fonts.

## Guides and examples

Start with the [documentation index](docs/README.md), or choose a topic:

- [Integration and incremental layout](docs/integration.md)
- [Japanese typography](docs/japanese-layout.md) and [vertical output](docs/vertical-layout.md)
- [Text paint](docs/paint-styles.md), [PNG rendering](docs/png-render-sample.md), and [emoji](docs/emoji.md)
- [Accessibility and AccessKit](docs/accessibility.md)
- [Fixed-font regression snapshots](docs/snapshot-tests.md) and [browser comparisons](docs/browser-comparison.md)

Generate API documentation locally with `cargo doc -p shodo --no-deps --open`.
For API documentation including the optional AccessKit adapter, add
`--features accesskit`.

To draw public glyph output with the checked-in fixture fonts:

```sh
cargo run -p shodo-fixtures --example render_png -- target/shodo-sample.png
```

The fixture and benchmark packages are development tools. Normal builds of the
root library do not include their fonts or rendering dependencies.

## Known limitations

`word-break: auto-phrase` warns and uses normal breaking. Automatic dictionary
hyphenation warns and uses manual soft-hyphen opportunities. Invalid locale tags
fall back to the root locale with a warning; unknown shaping languages use the
default OpenType language system with a warning.

The [float harness](docs/float-integration-harness.md) demonstrates caller placement
and retries, and the browser comparison records known differences. These checks
do not establish browser or WPT conformance. See individual guides for renderer,
emoji, and accessibility integration limits.

## Contributing

Bug reports, regression tests, documentation fixes, and implementation changes
are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for setup, CI checks, fixture
updates, and pull request guidance. Report reproducible bugs or propose features
in [GitHub Issues](https://github.com/fulgur-rs/shodo/issues).

## License

Code and original sample text are licensed under either
[Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
Bundled development font assets have separate SIL Open Font License 1.1 notices;
see the [fixture provenance and licenses](dev/fixtures/README.md).
