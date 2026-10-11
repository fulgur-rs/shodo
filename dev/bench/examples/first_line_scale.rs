//! First-line paragraph cost and lossless output probe.
//!
//! `cargo run --release -p shodo-bench --example first_line_scale -- [words]`
//! prints per-case timings and a SHA-256 of the public line output, so a
//! change can be checked for identical output while timing the
//! `::first-line` overhead.
#[path = "support/ligature_snapshot.rs"]
mod snapshot;
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, LineOptions, OverflowWrap, ParagraphStyle, TextTransform, WordBreak,
};
use shodo::{
    AtomicIntrinsics, AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};
use std::time::Instant;

const WORDS: &[&str] = &[
    "office",
    "first",
    "affinity",
    "flow",
    "the",
    "fluffy",
    "waffle",
    "of",
    "difficult",
    "effort",
    "and",
    "official",
    "fjord",
    "shuffle",
    "to",
    "baffling",
    "in",
    "fifty",
    "offline",
    "staff",
    "profile",
    "a",
    "efficient",
    "suffix",
    "with",
    "field",
];

#[derive(Clone, Copy)]
enum Variant {
    None,
    NoneAnywhere,
    Size,
    Same,
    Upper,
    Anywhere,
    BreakAll,
}

fn style(variant: Variant) -> ParagraphStyle {
    let mut s = ParagraphStyle::default();
    s.root.font_size = 16.;
    s.root.font_families = vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())];
    if matches!(variant, Variant::Anywhere | Variant::NoneAnywhere) {
        s.root.overflow_wrap = OverflowWrap::Anywhere;
    }
    if matches!(variant, Variant::BreakAll) {
        s.root.word_break = WordBreak::BreakAll;
    }
    let mut first = s.root.clone();
    match variant {
        Variant::None | Variant::NoneAnywhere => return s,
        Variant::Same => {}
        Variant::Size | Variant::Anywhere | Variant::BreakAll => first.font_size = 22.,
        Variant::Upper => {
            first.font_size = 20.;
            first.text_transform = TextTransform::Uppercase;
        }
    }
    s.first_line = Some(first);
    s
}

fn text(words: usize) -> String {
    let mut out = String::new();
    for i in 0..words {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(WORDS[(i * 7 + i / 3) % WORDS.len()]);
    }
    out
}

fn layout(
    p: &Paragraph,
    width: f32,
    cx: &mut LayoutContext,
    max_graphemes: Option<usize>,
) -> Vec<LineResult> {
    let mut token = p.start_token();
    let mut out = Vec::new();
    loop {
        let mut con = LineConstraint::new(width);
        con.max_graphemes = max_graphemes;
        let r = p.next_line(
            cx,
            token,
            &LineOptions::default(),
            &con,
            &AtomicSizes::EMPTY,
        );
        match &r {
            LineResult::Line(l) => token = l.break_token(),
            LineResult::Done => {
                out.push(r);
                return out;
            }
            other => panic!("unexpected {other:?}"),
        }
        out.push(r);
        if max_graphemes.is_some() && out.len() >= 3 {
            return out;
        }
    }
}

fn main() {
    let words: usize = std::env::args()
        .nth(1)
        .map_or(4000, |s| s.parse().expect("word count"));
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    let source = text(words);
    let mut total = Sha256::new();
    for (name, variant) in [
        ("none", Variant::None),
        ("none-any", Variant::NoneAnywhere),
        ("same", Variant::Same),
        ("size", Variant::Size),
        ("upper", Variant::Upper),
        ("anywhere", Variant::Anywhere),
        ("break-all", Variant::BreakAll),
    ] {
        let s = style(variant);
        let mut b = ParagraphBuilder::new(&s, &Limits::default());
        b.with_offset_mapping(true);
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            &source,
        );
        let t = Instant::now();
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let build = t.elapsed();
        let mut hash = Sha256::new();
        let mut break_time = std::time::Duration::ZERO;
        for width in [37.5_f32, 120., 400., 1000.] {
            let mut cx = LayoutContext::new();
            let t = Instant::now();
            let out = layout(&p, width, &mut cx, None);
            break_time += t.elapsed();
            // Break tokens are opaque; their unit index tracks internal
            // slicing, so compare the visible output only.
            let mut value = snapshot::results(&out);
            for line in value.as_array_mut().unwrap() {
                line.as_object_mut().unwrap().remove("token");
            }
            let json = value.to_string();
            if let Ok(dir) = std::env::var("FIRST_LINE_DUMP") {
                std::fs::write(format!("{dir}/{name}-{width}.json"), &json).unwrap();
            }
            hash.update(json);
            let w = format!("{:?}", cx.take_warnings());
            if let Ok(dir) = std::env::var("FIRST_LINE_DUMP") {
                std::fs::write(format!("{dir}/{name}-{width}-w.txt"), &w).unwrap();
            }
            hash.update(w);
        }
        for limit in [1, 2, 3, 5] {
            let out = layout(&p, 400., &mut LayoutContext::new(), Some(limit));
            let mut value = snapshot::results(&out);
            for line in value.as_array_mut().unwrap() {
                line.as_object_mut().unwrap().remove("token");
            }
            if let Ok(dir) = std::env::var("FIRST_LINE_DUMP") {
                std::fs::write(format!("{dir}/{name}-g{limit}.json"), value.to_string()).unwrap();
            }
            hash.update(value.to_string());
        }
        let t = Instant::now();
        let sizes = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &AtomicIntrinsics::EMPTY,
        );
        let intrinsic = t.elapsed();
        if let Ok(dir) = std::env::var("FIRST_LINE_DUMP") {
            std::fs::write(
                format!("{dir}/{name}-misc.txt"),
                format!("{sizes:?}\n{:?}", p.warnings()),
            )
            .unwrap();
        }
        hash.update(format!("{sizes:?}{:?}", p.warnings()));
        let digest = hash.finalize();
        total.update(digest);
        println!(
            "{name:10} build {:8.2} ms  break(4 widths) {:8.2} ms  intrinsic {:8.2} ms  out {:x}",
            build.as_secs_f64() * 1e3,
            break_time.as_secs_f64() * 1e3,
            intrinsic.as_secs_f64() * 1e3,
            digest
        );
    }
    println!("total {:x}", total.finalize());
}
