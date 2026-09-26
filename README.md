# shodo

A Rust text typesetting library for [raikiri](https://github.com/fulgur-rs/raikiri), intended as a replacement for parley. shodo lays out the inline content of a single block container (a paragraph).

shodo is in early development. Paragraph construction, whitespace processing, bidirectional text, basic line breaking, and fragment output are implemented, but typesetting with real fonts is not yet supported. Public APIs may change.

## Current implementation

- `ParagraphBuilder` for text, nested inline boxes, atomic inlines such as images, and forced line breaks.
- `RichText` for styled text without a DOM.
- Whitespace processing across nodes and mapping back to UTF-8 byte offsets in the source text with `OffsetMapping`.
- Bidirectional text analysis using `unicode-bidi` and visual ordering within lines.
- Greedy line breaking, tabs, indentation, participating inline/atomic line boxes, vertical alignment, and height-limit retries through `Paragraph::next_line`.
- Alignment and justification without copying shared glyphs, plus cluster views with adjusted advances.
- Incremental float reporting and withdrawal, with a single bounded partial-line cache.
- Fixed-width `break_all`, callback-driven `lines`, intrinsic widths with atomic/float inputs, and `balance` / `pretty` break plans.
- Read access to glyph runs, inline boxes, atomic inlines, and anchors for out-of-flow elements.
- Logical coordinates (inline / block axes) and conversion to physical coordinates with `PhysicalConverter`.
- Resource limits for input, glyph counts, and font data through `Limits`, plus diagnostic warnings.

### Limitations and planned features

Shaping is a placeholder: it generally produces one glyph per character with a 1em advance. Font matching and metrics are also placeholders, so registering a font does not yet affect actual glyph shapes or character widths.

Soft line break opportunities are currently limited to spaces, tabs, and atomic inlines. Unicode line breaking (UAX #14) and Japanese line breaking restrictions are not yet supported.

Some style and result types reserve future functionality. `::first-line`, real shaping/font matching, full CSS spacing, hyphenation, hit testing, vertical shaping, and ruby are not implemented. Line-edge reshaping currently uses the stub shaper; it does not provide real-script contextual shaping. APIs described in the design documents are not necessarily implemented.

Float placement remains the caller's responsibility. The protocol reports anchors and displaced floats; it is not a BFC or a production renderer integration. A `BreakPlan` is ignored when its paragraph, width, options, atomic revision, or float constraints do not match.

## Getting started

Requires Rust 1.89.0 or later (edition 2024).

```sh
git clone https://github.com/fulgur-rs/shodo.git
cd shodo
cargo build
cargo test
```

To try shodo from another local project, add a path dependency to its `Cargo.toml`. Adjust the path to match your checkout.

```toml
[dependencies]
shodo = { path = "../shodo" }
```

## Usage

Build a paragraph and lay it out one line at a time at a given width. This example demonstrates the layout API; character widths currently come from the placeholder shaper.

```rust
use shodo::font::FontCollection;
use shodo::limits::Limits;
use shodo::style::{InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult, RichText};

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

    let options = LineOptions::default();
    let mut constraint = LineConstraint::new(160.0);
    let mut token = paragraph.start_token();

    loop {
        match paragraph.next_line(
            &mut cx,
            token,
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) {
            LineResult::Line(line) => {
                println!("{}", &paragraph.text()[line.text_range()]);
                token = line.break_token();
                constraint.block_offset += line.block_size();
            }
            LineResult::Done => break,
            other => return Err(format!("Unexpected layout result: {other:?}").into()),
        }
    }

    Ok(())
}
```

Once built, a `Paragraph` is immutable, and clones share its data. As long as the text, paragraph style, inline styles, and font generations remain unchanged, you can reuse the paragraph with different widths and line layout constraints. A `BreakToken` is valid only for the paragraph that produced it.

For fixed-width content without float placement, the loop can be replaced with:

```rust,ignore
let lines = paragraph.break_all(&mut cx, &options, 160.0, &AtomicSizes::EMPTY);
for line in &lines {
    println!("{}", &paragraph.text()[line.text_range()]);
}
```

`break_all` keeps floats as zero-width anchors and splits at block boundaries. For incremental float layout, retain the cursor across lines, retry from `line_start`, and withdraw displaced floats in reverse order, one at a time. Re-reported withdrawn floats must be deferred for that line. Accept only lines with no displaced floats, and save/restore token, cursor, placements, deferred floats, and withdrawal records together when discarding lookahead or moving to another page.

`lines` handles `FloatEncountered` and `BlockSizeExceeded` through its constraint callback. The callback must change the constraint to make progress; retry an over-tall first-page line with `max_block_size: None`. `LayoutContext::shrink_to(0)` releases the retained partial line.

For DOM integration, pass `NodeId` and `TextSource` values to `ParagraphBuilder`. The caller computes sizes and baselines for images and other atomic inlines and supplies them through `AtomicSizes`. Rendering is also the caller's responsibility; use `Line::fragments()` to read the layout output.

Read build warnings through `Paragraph::warnings()` and line layout warnings through `LayoutContext::take_warnings()`. Resource limit violations are returned as `LimitExceeded`.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
```

## Design documents

- [Foundation design](docs/superpowers/specs/2026-09-26-shodo-foundation-design.md): intended APIs, responsibilities, and feature designs (in Japanese).
- [S0a implementation plan](docs/superpowers/plans/2026-09-26-shodo-s0a-skeleton-core.md): implementation plan for the initial core (in Japanese).

## License

Licensed under either of the following, at your option:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))
