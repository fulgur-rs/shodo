//! Table-cell workload: measure every cell's intrinsic sizes first, then lay
//! out every cell at its column width (the raikiri table order).
//!
//! Run with `cargo run --release -p shodo-bench --example table_cells -- [font] [rows] [samples]`.
//! Paragraph construction is outside the timed regions.
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::limits::Limits;
use shodo::style::{FontFamily, LineOptions, ParagraphStyle};
use shodo::{AtomicIntrinsics, AtomicSizes, LayoutContext, Paragraph, RichText};
use std::hint::black_box;
use std::time::Instant;

fn fonts(path: &str, limits: &Limits) -> FontCollection {
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            std::fs::read(path).expect("font file"),
            0,
            FontFaceDescriptor {
                family: "Probe".into(),
                ..Default::default()
            },
        )
        .expect("registered face");
    fonts
}

fn cell(row: usize, column: usize) -> String {
    match column {
        0 => format!("Item {row}"),
        1 => {
            let n = row * 37 % 100_000;
            if n >= 1000 {
                format!("{},{:03}", n / 1000, n % 1000)
            } else {
                n.to_string()
            }
        }
        _ => format!("Note {} ok", row % 97),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let font = args
        .next()
        .unwrap_or_else(|| "/usr/share/fonts/noto/NotoSans-Regular.ttf".into());
    let rows: usize = args.next().map_or(20_000, |a| a.parse().expect("rows"));
    let samples: usize = args.next().map_or(5, |a| a.parse().expect("samples"));
    let limits = Limits::default();
    let fonts = fonts(&font, &limits);
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Probe".into())];
    style.root.font_size = 13.333;
    let options = LineOptions::default();
    let mut cx = LayoutContext::new();
    let cells: Vec<Paragraph> = (0..rows)
        .flat_map(|row| (0..3).map(move |column| (row, column)))
        .map(|(row, column)| {
            RichText::with_limits(&style, &limits)
                .push(&cell(row, column), &style.root)
                .build(&mut cx, &fonts)
                .expect("paragraph")
        })
        .collect();
    for _ in 0..samples {
        let start = Instant::now();
        let widths: Vec<f32> = cells
            .iter()
            .map(|p| {
                p.intrinsic_sizes(&mut cx, &options, &AtomicIntrinsics::EMPTY)
                    .max_content
            })
            .collect();
        let intrinsic = start.elapsed();
        let start = Instant::now();
        let mut lines = 0usize;
        for (p, width) in cells.iter().zip(&widths) {
            let out = p.break_all(&mut cx, &options, *width, &AtomicSizes::EMPTY);
            lines += out.len();
            black_box(out);
        }
        let layout = start.elapsed();
        println!(
            "cells={} intrinsic_ms={:.1} layout_ms={:.1} lines={lines}",
            cells.len(),
            intrinsic.as_secs_f64() * 1e3,
            layout.as_secs_f64() * 1e3
        );
    }
}
