//! Public-API probe for soft-hyphen break cost. Usage:
//! `<plain|shy> <words> <reps>`. Prints break and intrinsic timings plus a
//! hash of the lossless line output and intrinsic sizes.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineHeight, LineOptions, ParagraphStyle};
use shodo::{
    AtomicIntrinsics, AtomicSizes, LayoutContext, Line, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};
use std::hint::black_box;
use std::time::Instant;

fn text(shy: bool, words: usize) -> String {
    const WORDS: [&str; 6] = [
        "hyphenation",
        "paragraphs",
        "breakable",
        "discretion",
        "measurement",
        "candidate",
    ];
    let mut out = String::new();
    for i in 0..words {
        let w = WORDS[i % WORDS.len()];
        for (j, c) in w.chars().enumerate() {
            if shy && j > 0 && j % 3 == 0 {
                out.push('\u{ad}');
            }
            out.push(c);
        }
        out.push(' ');
    }
    out
}

fn layout(p: &Paragraph, cx: &mut LayoutContext, width: f32) -> Vec<Line> {
    let constraint = LineConstraint::new(width);
    let mut token = p.start_token();
    let mut lines = Vec::new();
    loop {
        match p.next_line(
            cx,
            token,
            &LineOptions::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) {
            LineResult::Line(line) => {
                token = line.break_token();
                lines.push(line);
            }
            LineResult::Done => break,
            other => panic!("unexpected layout result: {other:?}"),
        }
    }
    lines
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args.len(), 3, "use: <plain|shy> <words> <reps>");
    assert!(
        ["plain", "shy"].contains(&args[0].as_str()),
        "use: <plain|shy> <words> <reps>"
    );
    let words: usize = args[1].parse().unwrap();
    let reps: usize = args[2].parse().unwrap();
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let style = InlineStyle {
        font_size: 10.0,
        line_height: LineHeight::Px(12.0),
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())],
        ..Default::default()
    };
    let ps = ParagraphStyle {
        root: style,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&ps, &limits);
    b.push_text(
        TextSource::Generated { node: NodeId(1) },
        &text(args[0] == "shy", words),
    );
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let widths = [61.0f32, 97.0, 233.0, 451.0];
    let mut cx = LayoutContext::new();
    let mut out = Vec::new();
    for w in widths {
        out.push(json!(
            layout(&p, &mut cx, w)
                .iter()
                .map(snapshot::line)
                .collect::<Vec<_>>()
        ));
    }
    let sizes = p.intrinsic_sizes(
        &mut cx,
        &LineOptions::default(),
        &AtomicIntrinsics::default(),
    );
    out.push(json!(format!("{sizes:?}")));
    let warnings: Vec<_> = cx
        .take_warnings()
        .iter()
        .map(|w| format!("{:?}:{}", w.kind, w.message))
        .collect();
    out.push(json!(warnings));
    let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&out).unwrap()));
    let started = Instant::now();
    for _ in 0..reps {
        for w in widths {
            black_box(layout(&p, &mut LayoutContext::new(), w));
        }
    }
    let break_ns = started.elapsed().as_nanos() / reps as u128;
    let started = Instant::now();
    for _ in 0..reps {
        black_box(p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &AtomicIntrinsics::default(),
        ));
    }
    let intrinsic_ns = started.elapsed().as_nanos() / reps as u128;
    println!(
        "{}",
        json!({"case":args[0],"words":words,"break_ms":break_ns as f64/1e6,"intrinsic_ms":intrinsic_ns as f64/1e6,"sha256":hash,"sizes":format!("{sizes:?}"),"warnings":warnings})
    );
}
