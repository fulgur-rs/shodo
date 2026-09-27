//! Inspect horizontal layout and hit geometry. Pass an optional sfnt font path;
//! otherwise the deterministic missing-font fallback demonstrates the API.
//! This example prints geometry; it does not rasterize glyphs or decorations.
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::geometry::{Direction, PhysicalConverter, PhysicalSize, WritingMode};
use shodo::hit::{CaretDirection, LineLayout, NavigationOrder, TextPosition};
use shodo::limits::Limits;
use shodo::mapping::Affinity;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mut style = ParagraphStyle::default();
    style.root.font_size = 20.0;
    style.root.letter_spacing = 1.0;
    if let Some(path) = std::env::args_os().nth(1) {
        fonts.register_face(
            std::fs::read(path)?,
            0,
            FontFaceDescriptor {
                family: "Example".into(),
                ..Default::default()
            },
        )?;
        style.root.font_families = vec![FontFamily::Named("Example".into())];
    }
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.with_offset_mapping(true).push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "office a b c",
    );
    let mut context = LayoutContext::new();
    let paragraph = builder.build(&mut context, &fonts)?;
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        100.0,
        &AtomicSizes::EMPTY,
    );
    drop(paragraph);
    drop(fonts); // accepted Lines retain the actual font layer
    for line in &lines {
        println!(
            "text={:?} metrics={:?} nominal_ink={:?} hanging={}",
            &line.text()[line.text_range()],
            line.metrics(),
            line.overflow_rect(),
            line.hang_end()
        );
        for fragment in line.fragments() {
            if let Fragment::GlyphRun(run) = fragment {
                println!(
                    "font={:?} size={} coords={:?} glyphs={:?}",
                    run.font(),
                    run.font_size(),
                    run.normalized_coords(),
                    run.glyphs().collect::<Vec<_>>()
                );
            }
        }
    }
    let layout = LineLayout::new(&lines);
    let hit = layout.hit_test(25.0, 10.0).ok_or("empty layout")?;
    let caret = layout.caret(hit.position).ok_or("missing caret")?;
    let last = lines.last().ok_or("empty layout")?;
    let end = TextPosition {
        line: lines.len() - 1,
        offset: last.text_range().end as u32,
        affinity: Affinity::Upstream,
    };
    let selection = layout.selection_rects(hit.position, end);
    let converter = PhysicalConverter::new(
        WritingMode::HorizontalTb,
        Direction::Ltr,
        PhysicalSize {
            width: 100.0,
            height: lines.iter().map(|l| l.block_size()).sum(),
        },
    );
    // Hit/selection already include block_offset; do not add it a second time.
    println!("hit={hit:?} caret={:?}", converter.rect(caret.rect));
    println!(
        "selection={:?}",
        selection
            .into_iter()
            .map(|r| converter.rect(r))
            .collect::<Vec<_>>()
    );
    println!(
        "logical_next={:?} visual_next={:?}",
        layout.move_caret(
            hit.position,
            CaretDirection::Forward,
            NavigationOrder::Logical
        ),
        layout.move_caret(
            hit.position,
            CaretDirection::Forward,
            NavigationOrder::Visual
        )
    );
    Ok(())
}
