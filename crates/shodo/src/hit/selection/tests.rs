use super::*;
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::geometry::{Direction, WritingMode};
use crate::mapping::Affinity;
use crate::node::{NodeId, TextSource};
use crate::style::{FontFamily, ParagraphStyle, TabSize, TextCombineUpright, WhiteSpaceCollapse};
use crate::{AtomicSize, AtomicSizes, LayoutContext, Line, ParagraphBuilder};

std::thread_local! {static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };}
pub(crate) fn visit() {
    WORK.with(|work| work.set(work.get() + 1));
}
fn take_work() -> usize {
    WORK.with(|work| work.replace(0))
}

fn lines(mode: &str, count: usize) -> Vec<Line> {
    let limits = Default::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (family, bytes) in [
        ("Latin", crate::test_support::fonts::LATIN),
        ("Arabic", crate::test_support::fonts::ARABIC),
    ] {
        fonts
            .register_face(
                bytes.to_vec(),
                0,
                FontFaceDescriptor {
                    family: family.into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![
        FontFamily::Named("Latin".into()),
        FontFamily::Named("Arabic".into()),
    ];
    style.root.font_size = 16.0;
    style.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    style.root.tab_size = TabSize::Px(40.0);
    if mode == "rtl" {
        style.direction = Direction::Rtl;
    }
    if mode.starts_with("tcy") {
        style.writing_mode = if mode == "tcy-lr" {
            WritingMode::VerticalLr
        } else {
            WritingMode::VerticalRl
        };
    }
    if mode == "first-line" {
        let mut first = style.root.clone();
        first.font_size = 20.0;
        style.first_line = Some(first);
    }
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.with_offset_mapping(true);
    let source = |node| TextSource::Dom {
        node: NodeId(node),
        offset: 7,
    };
    let mut sizes = AtomicSizes::new();
    match mode {
        "plain" => {
            builder.push_text(source(1), &"a".repeat(count));
        }
        "mixed" => {
            builder
                .push_text(source(1), &"a".repeat(count / 2))
                .push_atomic(NodeId(2), &style.root, Default::default())
                .push_text(source(3), &format!("\tb{}", "a".repeat(count - count / 2)));
            sizes.insert(
                NodeId(2),
                AtomicSize {
                    inline_size: 13.0,
                    block_size: 21.0,
                    ..Default::default()
                },
            );
        }
        "bidi" | "rtl" => {
            builder.push_text(source(1), "ab مرحبا cd ");
        }
        "shared" => {
            for i in 0..count {
                let mut alternate = style.root.clone();
                alternate.font_size = 20.0;
                builder
                    .push_text(source(10 + i as u64 * 2), "a")
                    .open_inline(NodeId(20_000 + i as u64), &alternate, Default::default())
                    .push_text(source(11 + i as u64 * 2), "\u{301}")
                    .close_inline();
            }
        }
        "first-line" => {
            builder.push_text(source(1), "abc\ndef");
        }
        "empty" => {}
        _ if mode.starts_with("tcy") => {
            let mut combined = style.root.clone();
            combined.text_combine_upright = TextCombineUpright::All;
            for i in 0..count {
                builder
                    .open_inline(NodeId(10_000 + i as u64), &combined, Default::default())
                    .push_text(source(i as u64 + 1), "12")
                    .close_inline();
            }
        }
        _ => panic!("unknown test fixture"),
    }
    let mut cx = LayoutContext::new();
    let paragraph = builder.build(&mut cx, &fonts).unwrap();
    assert!(cx.take_warnings().is_empty());
    let lines = paragraph.break_all(&mut cx, &Default::default(), 1_000_000.0, &sizes);
    assert!(cx.take_warnings().is_empty());
    lines
}
fn pos(line: usize, offset: u32, affinity: Affinity) -> TextPosition {
    TextPosition {
        line,
        offset,
        affinity,
    }
}

#[test]
fn short_queries_bound_actual_source_visits() {
    let mut work = Vec::new();
    for mode in ["plain", "mixed"] {
        for count in [1024, 4096, 16384] {
            let lines = lines(mode, count);
            let layout = LineLayout::new(&lines);
            let a = pos(0, 7, Affinity::Downstream);
            let b = pos(0, 8, Affinity::Upstream);
            let left = layout.caret(a).unwrap().rect;
            let right = layout.caret(b).unwrap().rect;
            let expected = LogicalRect {
                inline_size: right.inline_start - left.inline_start,
                ..left
            };
            take_work();
            assert_eq!(layout.selection_rects(a, b), vec![expected]);
            let visits = take_work();
            println!(
                "SELECTION_WORK {mode}/{count}: {visits} visited, exact one-character rectangle passed"
            );
            work.push(visits);
        }
    }
    assert!(
        work.iter().all(|n| *n <= 64),
        "short queries must avoid full source scans, actual {work:?}"
    );
}

// Original source predicate and visual merge phases, intentionally independent
// of candidate index construction/pruning. Semantic oracle for accepted lines.
fn linear(layout: &LineLayout<'_>, a: TextPosition, b: TextPosition) -> Vec<LogicalRect> {
    let (Some(a), Some(b)) = (layout.caret(a), layout.caret(b)) else {
        return Vec::new();
    };
    let (mut a, mut b) = (a.position, b.position);
    if (a.line, a.offset) > (b.line, b.offset) {
        std::mem::swap(&mut a, &mut b);
    }
    if (a.line, a.offset) == (b.line, b.offset) {
        return Vec::new();
    };
    let mut result = Vec::new();
    for line in a.line..=b.line {
        let from = if line == a.line { a.offset } else { 0 };
        let to = if line == b.line { b.offset } else { u32::MAX };
        let mut rects: Vec<_> = layout.index[line]
            .segments
            .iter()
            .filter(|s| {
                s.text.start < to
                    && s.text.end > from
                    && s.rect.inline_size > 0.0
                    && s.rect.block_size > 0.0
            })
            .map(|s| s.rect)
            .collect();
        rects.sort_by(|a, b| {
            a.block_start
                .total_cmp(&b.block_start)
                .then(a.block_size.total_cmp(&b.block_size))
                .then(a.inline_start.total_cmp(&b.inline_start))
        });
        let mut merged: Vec<LogicalRect> = Vec::new();
        for rect in rects {
            if let Some(previous) = merged.last_mut()
                && previous.block_start == rect.block_start
                && previous.block_size == rect.block_size
                && rect.inline_start <= previous.inline_start + previous.inline_size
            {
                previous.inline_size = (rect.inline_start + rect.inline_size)
                    .max(previous.inline_start + previous.inline_size)
                    - previous.inline_start;
            } else {
                merged.push(rect);
            }
        }
        merged.sort_by(|a, b| {
            a.inline_start
                .total_cmp(&b.inline_start)
                .then(a.inline_size.total_cmp(&b.inline_size))
                .then(a.block_start.total_cmp(&b.block_start))
        });
        let mut final_rects: Vec<LogicalRect> = Vec::new();
        for rect in merged {
            if let Some(previous) = final_rects.last_mut()
                && previous.inline_start == rect.inline_start
                && previous.inline_size == rect.inline_size
                && rect.block_start <= previous.block_start + previous.block_size
            {
                previous.block_size = (rect.block_start + rect.block_size)
                    .max(previous.block_start + previous.block_size)
                    - previous.block_start;
            } else {
                final_rects.push(rect);
            }
        }
        result.extend(final_rects);
    }
    result
}
#[test]
fn source_candidate_search_preserves_all_accepted_selection_rectangles() {
    for mode in [
        "plain",
        "mixed",
        "bidi",
        "rtl",
        "shared",
        "first-line",
        "tcy-rl",
        "tcy-lr",
        "empty",
    ] {
        let lines = lines(mode, 8);
        let layout = LineLayout::new(&lines);
        let mut positions = Vec::new();
        for (line, index) in layout.index.iter().enumerate() {
            positions.extend(index.stops.iter().map(|stop| stop.position));
            positions.push(pos(
                line,
                index.stops.first().map_or(0, |s| s.position.offset) + 1,
                Affinity::Downstream,
            ));
        }
        positions.push(pos(lines.len(), u32::MAX, Affinity::Upstream));
        for &a in &positions {
            for &b in &positions {
                assert_eq!(
                    layout.selection_rects(a, b),
                    linear(&layout, a, b),
                    "{mode} {a:?}..{b:?}"
                );
            }
        }
    }
}

#[test]
#[ignore = "prints actual accepted source segment order and duplicate geometry"]
fn selection_source_order_diagnostic() {
    for mode in [
        "plain", "mixed", "bidi", "rtl", "shared", "tcy-rl", "tcy-lr",
    ] {
        let lines = lines(mode, 16);
        let layout = LineLayout::new(&lines);
        for index in &layout.index {
            let segments = &index.segments;
            let ordered = segments
                .windows(2)
                .all(|s| s[0].text.start <= s[1].text.start && s[0].text.end <= s[1].text.end);
            let duplicates = segments
                .iter()
                .enumerate()
                .filter(|(i, s)| segments[..*i].iter().any(|a| a.text == s.text))
                .count();
            println!(
                "SOURCE_ORDER mode={mode} segments={} monotonic={ordered} duplicate_ranges={duplicates}",
                segments.len()
            );
            if mode == "plain" {
                assert!(ordered);
            }
            if mode == "mixed" {
                assert!(!ordered);
            }
        }
    }
}

#[test]
fn bidi_partial_selection_keeps_the_visual_gap() {
    let lines = lines("bidi", 1);
    let layout = LineLayout::new(&lines);
    // "ab " then only the first Arabic source character at the right of its run.
    let a = pos(0, 0, Affinity::Downstream);
    let b = pos(0, 5, Affinity::Upstream);
    let rects = layout.selection_rects(a, b);
    assert_eq!(rects.len(), 2);
    assert!(rects[0].inline_start + rects[0].inline_size < rects[1].inline_start);
    assert_eq!(layout.selection_rects(b, a), rects);
}

#[test]
fn full_selection_does_not_traverse_the_interval_tree_before_every_candidate() {
    let lines = lines("mixed", 16384);
    let layout = LineLayout::new(&lines);
    let a = pos(0, 0, Affinity::Downstream);
    let b = pos(0, lines[0].text_range().end as u32, Affinity::Upstream);
    let expected = linear(&layout, a, b);
    take_work();
    assert_eq!(layout.selection_rects(a, b), expected);
    let work = take_work();
    let segments = layout.index[0].segments.len();
    assert!(
        work <= segments + 64,
        "full selection must avoid extra tree visits, {work} visits for {segments} candidates"
    );
}
