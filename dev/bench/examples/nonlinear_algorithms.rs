//! Targeted before/after probe. See tools/bench/compare_nonlinear.py.
//! Callgrind collects only the two explicitly named measurement functions.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::hit::{LineLayout, TextPosition};
use shodo::limits::Limits;
use shodo::mapping::Affinity;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle, UnicodeBidi, WhiteSpaceCollapse};
use shodo::{LayoutContext, Line, ParagraphAnalysis, ParagraphBuilder, RubyAnnotationView};
use std::hint::black_box;
use std::time::Instant;

#[path = "support/ruby_hit_fixture.rs"]
mod fixture;

struct Whitespace {
    depth: usize,
    active: bool,
    text: String,
    style: ParagraphStyle,
    limits: Limits,
}

fn analyze(input: &Whitespace) -> ParagraphAnalysis {
    let mut builder = ParagraphBuilder::new(&input.style, &input.limits);
    for i in 0..input.depth {
        let style = InlineStyle {
            unicode_bidi: if input.active && i == 0 {
                UnicodeBidi::Isolate
            } else if input.active && i + 1 == input.depth {
                UnicodeBidi::Embed
            } else {
                UnicodeBidi::Normal
            },
            ..input.style.root.clone()
        };
        builder.open_inline(NodeId(i as u64), &style, InlineEdges::default());
    }
    builder.push_text(
        TextSource::Dom {
            node: NodeId(10_000),
            offset: 0,
        },
        &input.text,
    );
    for _ in 0..input.depth {
        builder.close_inline();
    }
    builder.analyze().expect("valid bounded analysis input")
}

// Stable symbols permit --toggle-collect without a Valgrind client dependency.
// SAFETY: these two probe-specific exported names are unique to this binary.
#[unsafe(no_mangle)]
#[inline(never)]
fn shodo_whitespace_measure(input: &Whitespace, iterations: usize) -> ParagraphAnalysis {
    let mut last = None;
    for _ in 0..iterations {
        last = Some(black_box(analyze(black_box(input))));
    }
    last.unwrap()
}

#[unsafe(no_mangle)]
#[inline(never)]
fn shodo_ruby_measure(layout: &LineLayout<'_>, point: (f32, f32), iterations: usize) {
    for _ in 0..iterations {
        let hit = layout
            .hit_test_ruby(black_box(point.0), black_box(point.1))
            .expect("deepest annotation remains hittable");
        black_box(hit);
    }
}

fn body_point(line: &Line) -> (f32, f32) {
    let layout = LineLayout::new(std::slice::from_ref(line));
    let caret = layout
        .caret(TextPosition {
            line: 0,
            offset: line.text_range().start as u32,
            affinity: Affinity::Downstream,
        })
        .unwrap();
    (
        line.inline_size() / 2.0,
        caret.rect.block_start + caret.rect.block_size / 2.0,
    )
}

fn deepest_point(lines: &[Line], depth: usize) -> (f32, f32) {
    let mut line = &lines[0];
    let mut path: Vec<(RubyAnnotationView<'_>, f32)> = Vec::new();
    while let Some(annotation) = line.ruby_annotations().next() {
        path.push((annotation, line.block_offset()));
        line = annotation.line();
    }
    assert_eq!(path.len(), depth);
    let mut point = body_point(line);
    for (annotation, parent_offset) in path.into_iter().rev() {
        let t = annotation.transform();
        point = (
            t.inline_inline * point.0 + t.inline_block * point.1 + t.inline_offset,
            t.block_inline * point.0 + t.block_block * point.1 + t.block_offset + parent_offset,
        );
    }
    point
}

fn ruby_digest(layout: &LineLayout<'_>, point: (f32, f32), depth: usize) -> Value {
    let hit = layout.hit_test_ruby(point.0, point.1).unwrap();
    let nodes: Vec<_> = hit.path().iter().map(|a| a.node().unwrap().0).collect();
    let expected: Vec<_> = (1..=depth).rev().map(|d| 200 + d as u64).collect();
    assert_eq!(nodes, expected);
    assert_eq!(hit.annotation.node(), Some(NodeId(201)));
    json!({
        "path_nodes": nodes,
        "parent_line": hit.parent_line(),
        "hit": format!("{:?}", hit.hit),
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: nonlinear_algorithms CASE ITERATIONS".into());
    }
    let case = &args[0];
    let iterations: usize = args[1].parse()?;
    if iterations == 0 {
        return Err("iterations must be positive".into());
    }
    let (elapsed, output) = if let Some(depth) = case.strip_prefix("ruby-") {
        let depth: usize = depth.parse()?;
        let (lines, warnings) = fixture::fixture(depth, &Limits::default())?;
        assert_eq!(warnings, "[]/[]");
        assert_eq!(fixture::actual_depth(&lines), depth);
        fixture::assert_real_glyphs(&lines);
        let point = deepest_point(&lines, depth);
        let layout = LineLayout::new(&lines);
        let expected = ruby_digest(&layout, point, depth);
        // Build all lazy indexes and warm the exact query without entering
        // either Callgrind measurement symbol.
        for _ in 0..5 {
            black_box(layout.hit_test_ruby(point.0, point.1).unwrap());
        }
        let begin = Instant::now();
        shodo_ruby_measure(&layout, point, iterations);
        let elapsed = begin.elapsed();
        assert_eq!(ruby_digest(&layout, point, depth), expected);
        (elapsed, expected)
    } else {
        let parts: Vec<_> = case.split('-').collect();
        if parts.len() != 3 || !matches!(parts[0], "ws" | "mixed") {
            return Err("case must be ws-DEPTH-BREAKS, mixed-DEPTH-BREAKS or ruby-DEPTH".into());
        }
        let depth = parts[1].parse()?;
        let breaks = parts[2].parse()?;
        let input = Whitespace {
            depth,
            active: parts[0] == "mixed",
            text: "a\nb".repeat(breaks),
            style: ParagraphStyle {
                root: InlineStyle {
                    white_space_collapse: WhiteSpaceCollapse::Preserve,
                    font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
                    ..Default::default()
                },
                ..Default::default()
            },
            limits: Limits::default(),
        };
        for _ in 0..5 {
            black_box(analyze(&input));
        }
        let begin = Instant::now();
        let analysis = shodo_whitespace_measure(&input, iterations);
        let elapsed = begin.elapsed();
        // Shaping and output validation stay outside the timed/collected
        // region. No line layout that would inflate RSS is needed here.
        let fonts = shodo_fixtures::load_fonts(&input.limits)?;
        let paragraph = analysis.shape(&mut LayoutContext::new(), &fonts.collection)?;
        let expected = if input.active {
            let (open, close) = ("\u{2066}\u{202A}", "\u{202C}\u{2069}");
            format!(
                "{open}{}{close}",
                input.text.replace('\n', &format!("{close}\n{open}"))
            )
        } else {
            input.text.clone()
        };
        assert_eq!(paragraph.text(), expected);
        let mut hash = Sha256::new();
        hash.update(paragraph.text().as_bytes());
        hash.update(format!("{:?}", paragraph.offset_mapping()).as_bytes());
        let output = json!({
            "sha256": format!("{:x}", hash.finalize()),
            "text_bytes": paragraph.text().len(),
            "warnings": format!("{:?}", paragraph.warnings()),
        });
        (elapsed, output)
    };
    let status = std::fs::read_to_string("/proc/self/status")?;
    let kib = |name| {
        status
            .lines()
            .find_map(|line| {
                line.strip_prefix(name)?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
            .expect("Linux process RSS is available")
    };
    println!(
        "{}",
        json!({
            "case": case,
            "iterations": iterations,
            "measured_ns": elapsed.as_nanos(),
            "rss_kib": kib("VmRSS:"),
            "peak_rss_kib": kib("VmHWM:"),
            "output": output,
        })
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
