//! Fixed-font accessibility layout build probe for long single and wrapped lines.
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::accessibility::AccessibleLayout;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, ParagraphStyle, TextWrapMode};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};
use std::hint::black_box;

fn digest(layout: &AccessibleLayout<'_>) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{:?}", layout.lines()).as_bytes())
    )
}

fn main() {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    for (characters, width) in [
        (512, 1_000_000.0),
        (4096, 1_000_000.0),
        (16384, 1_000_000.0),
        (16384, 128.0),
    ] {
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())];
        if width > 1_000.0 {
            style.root.text_wrap_mode = TextWrapMode::NoWrap;
        }
        let text = if width > 1_000.0 {
            "a".repeat(characters)
        } else {
            "a ".repeat(characters / 2)
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            &text,
        );
        let mut cx = LayoutContext::new();
        let paragraph = builder.build(&mut cx, &fonts.collection).unwrap();
        let lines = paragraph.break_all(&mut cx, &Default::default(), width, &AtomicSizes::EMPTY);
        let expected = digest(&AccessibleLayout::new(&lines));
        let mut samples = Vec::new();
        for sample in 0..10 {
            let start = std::time::Instant::now();
            let layout = AccessibleLayout::new(black_box(&lines));
            let elapsed = start.elapsed().as_nanos() as u64;
            black_box(layout.lines());
            assert_eq!(digest(&layout), expected);
            samples.push(json!({"sample":sample,"ns":elapsed}));
        }
        println!(
            "{}",
            json!({"characters":characters,"width":width,"lines":lines.len(),"digest":expected,"samples":samples})
        );
    }
}
