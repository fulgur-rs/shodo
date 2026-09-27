//! Fixed public-API layout -> accepted glyph/atomic output -> PNG.
#[path = "support/glyph_paint.rs"]
mod glyph_paint;
use shodo::hit::{LineLayout, TextPosition};
use shodo::mapping::Affinity;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{
    AtomicSize, AtomicSizes, LayoutContext, Line, LineConstraint, LineResult, ParagraphBuilder,
};
use shodo_fixtures::{FONTS, load_fonts};

fn sample() -> Result<Vec<Line>, Box<dyn std::error::Error>> {
    let limits = Default::default();
    let fonts = load_fonts(&limits)?;
    let latin = InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        font_size: 32.0,
        ..Default::default()
    };
    let arabic = InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[2].family.into())],
        ..latin.clone()
    };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: latin.clone(),
            ..Default::default()
        },
        &limits,
    );
    b.with_offset_mapping(true);
    for (i, text) in ["f", "f", "i"].iter().enumerate() {
        b.open_inline(NodeId(11 + i as u64), &latin, Default::default())
            .push_text(
                TextSource::Dom {
                    node: NodeId(1 + i as u64),
                    offset: 0,
                },
                text,
            )
            .close_inline();
    }
    b.push_text(
        TextSource::Dom {
            node: NodeId(4),
            offset: 0,
        },
        " ",
    )
    .push_atomic(NodeId(9), &latin, Default::default())
    .push_forced_break(NodeId(88))
    .open_inline(NodeId(15), &arabic, Default::default())
    .push_text(
        TextSource::Dom {
            node: NodeId(5),
            offset: 0,
        },
        "سلام",
    )
    .close_inline();
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(9),
        AtomicSize {
            inline_size: 32.0,
            block_size: 24.0,
            baseline: Some(24.0),
            margins: Default::default(),
        },
    );
    let mut cx = LayoutContext::new();
    let p = b.build(&mut cx, &fonts.collection)?;
    let mut token = p.start_token();
    let mut block = 0.0;
    let mut lines = Vec::new();
    loop {
        let mut constraint = LineConstraint::new(200.0);
        constraint.block_offset = block;
        match p.next_line(&mut cx, token, &Default::default(), &constraint, &atomics) {
            LineResult::Line(line) => {
                token = line.break_token();
                block += line.block_size();
                lines.push(line);
            }
            LineResult::Done => break,
            result => {
                return Err(std::io::Error::other(format!(
                    "unexpected sample layout result: {result:?}"
                ))
                .into());
            }
        }
    }
    Ok(lines)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("target/shodo-sample.png"));
    let lines = sample()?;
    let layout = LineLayout::new(&lines);
    let mapping = lines[0]
        .offset_mapping()
        .ok_or("sample requires offset mapping")?;
    let (a, _) = mapping
        .dom_to_text(NodeId(2), 0)
        .ok_or("middle ffi source absent")?;
    let (z, _) = mapping
        .dom_to_text(NodeId(2), 1)
        .ok_or("middle ffi source absent")?;
    let annotations = layout.selection_rects(
        TextPosition {
            line: 0,
            offset: a,
            affinity: Affinity::Downstream,
        },
        TextPosition {
            line: 0,
            offset: z,
            affinity: Affinity::Upstream,
        },
    );
    // Paint color is caller-owned. The shared ffi belongs to source1 (red),
    // despite source2's blue style and distinct selection/link region.
    let (image, glyphs) = glyph_paint::try_paint(
        &lines,
        |node| match node.0 {
            1 => [255, 0, 0, 255],
            2 => [0, 0, 255, 255],
            9 => [0, 128, 0, 255],
            _ => [0, 0, 0, 255],
        },
        &annotations,
    )?;
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    image.save_png(&output)?;
    println!(
        "{}: {} lines, {glyphs} accepted glyphs; middle ffi source region {:?}",
        output.display(),
        lines.len(),
        annotations
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_sample_retains_fonts_and_draws_glyphs_and_atomic_after_layout_owners_drop() {
        let lines = sample().unwrap();
        assert_eq!(lines.len(), 2);
        let (first, n) = glyph_paint::try_paint(&lines, |_| [0, 0, 0, 255], &[]).unwrap();
        assert_eq!(n, 5);
        assert_eq!(
            lines
                .iter()
                .flat_map(|l| l.fragments())
                .filter(|f| matches!(f, shodo::Fragment::Atomic(_)))
                .count(),
            1
        );
        for run in lines
            .iter()
            .flat_map(|l| l.fragments())
            .filter_map(|f| match f {
                shodo::Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
        {
            let data = run.font_data().unwrap();
            assert!(FONTS.iter().any(|f| f.bytes == data.data.as_ref()));
        }
        let (second, _) = glyph_paint::try_paint(&lines, |_| [0, 0, 0, 255], &[]).unwrap();
        assert_eq!(first.encode_png().unwrap(), second.encode_png().unwrap());
        assert_eq!(&first.encode_png().unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    }
}
