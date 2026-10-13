//! Repeated overflow queries; construction is excluded from measured time.
//! cargo run --release -p shodo-bench --example outline_bounds -- cjk 512 200
use shodo::geometry::Direction;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};
use std::{hint::black_box, time::Instant};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let case = args.get(1).map(String::as_str).unwrap_or("cjk");
    let count: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(512);
    let iterations: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(200);
    let fixture = shodo_fixtures::font(case).expect("latin, cjk, or arabic");
    let text = match case {
        "cjk" => "日本語文章。",
        "arabic" => "العربية ",
        _ => "office text ",
    }
    .repeat(count);
    let limits = Limits::unlimited();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let direction = if case == "arabic" {
        Direction::Rtl
    } else {
        Direction::Ltr
    };
    let style = ParagraphStyle {
        direction,
        root: InlineStyle {
            direction,
            font_families: vec![FontFamily::Named(fixture.family.into())],
            font_size: 16.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut cx = LayoutContext::new();
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, &text);
    let p = builder.build(&mut cx, &fonts.collection).unwrap();
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(1e6),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("line")
    };
    let start = Instant::now();
    let first = black_box(line.overflow_rect());
    let cold_ns = start.elapsed().as_nanos();
    let mut checksum = 0u64;
    let start = Instant::now();
    for _ in 0..iterations {
        let r = black_box(line.overflow_rect());
        checksum = checksum.wrapping_add(u64::from(r.inline_size.to_bits()));
    }
    let warm_ns = start.elapsed().as_nanos();
    println!(
        "{}",
        serde_json::json!({
            "case": case, "repeat": count, "iterations": iterations,
            "text_bytes": text.len(), "cold_ns": cold_ns, "warm_total_ns": warm_ns,
            "warm_ns_per_query": warm_ns as f64 / iterations as f64,
            "rect_bits": [first.inline_start.to_bits(), first.block_start.to_bits(),
                          first.inline_size.to_bits(), first.block_size.to_bits()], "checksum": checksum,
        })
    );
}
