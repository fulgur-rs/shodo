//! Generated-source inverse lookup benchmark. Keeps the former linear path as
//! an oracle so indexed lookups can be compared for exact position ordering.
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::accessibility::{AccessibleLayout, AccessiblePosition, SourcePosition};
use shodo::limits::Limits;
use shodo::mapping::{Affinity, OffsetMapping, TextOrigin};
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineBreak, ParagraphStyle, TextTransform, WhiteSpaceCollapse,
};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const SAMPLES: usize = 9;

fn linear_generated(
    layout: &AccessibleLayout<'_>,
    source: SourcePosition,
) -> Vec<AccessiblePosition> {
    let mut result = Vec::new();
    for line in 0..layout.lines().len() {
        for character in 0..=layout.lines()[line].characters.len() {
            if let Some(position) = layout.position(line, character, source.affinity)
                && layout
                    .to_source(position)
                    .is_some_and(|found| found.origin == source.origin)
                && result.last() != Some(&position)
            {
                result.push(position);
            }
        }
    }
    result.dedup();
    result
}

fn digest(results: &[Vec<AccessiblePosition>]) -> String {
    let positions = results
        .iter()
        .map(|items| {
            items
                .iter()
                .map(|position| {
                    (
                        position.line,
                        position.character,
                        position.affinity == Affinity::Downstream,
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&positions).unwrap())
    )
}

fn median(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn generated_lines(
    count: usize,
    limits: &Limits,
    fonts: &shodo::font::FontCollection,
) -> (Vec<shodo::Line>, Vec<NodeId>) {
    let style = InlineStyle {
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())],
        font_size: 16.0,
        line_break: LineBreak::Anywhere,
        ..Default::default()
    };
    let paragraph_style = ParagraphStyle {
        root: style.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, &limits);
    builder.with_offset_mapping(true);
    let nodes = (0..count)
        .map(|index| NodeId(10_000 + index as u64))
        .collect::<Vec<_>>();
    for &node in &nodes {
        builder.push_text(TextSource::Generated { node }, "x");
    }
    let mut context = LayoutContext::new();
    let paragraph = builder.build(&mut context, fonts).unwrap();
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        count as f32 * 64.0 + 1.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1);
    (lines, nodes)
}

fn dom_lines(
    count: usize,
    limits: &Limits,
    fonts: &shodo::font::FontCollection,
) -> (Vec<shodo::Line>, Vec<NodeId>) {
    let style = InlineStyle {
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())],
        font_size: 16.0,
        line_break: LineBreak::Anywhere,
        ..Default::default()
    };
    let paragraph_style = ParagraphStyle {
        root: style.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, &limits);
    builder.with_offset_mapping(true);
    let nodes = (0..count)
        .map(|index| NodeId(20_000 + index as u64))
        .collect::<Vec<_>>();
    for &node in &nodes {
        builder.push_text(TextSource::Dom { node, offset: 0 }, "x");
    }
    let mut context = LayoutContext::new();
    let paragraph = builder.build(&mut context, fonts).unwrap();
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        count as f32 * 64.0 + 1.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1);
    (lines, nodes)
}

fn edge_lines() -> Vec<shodo::Line> {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let root = InlineStyle {
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())],
        font_size: 16.0,
        line_break: LineBreak::Anywhere,
        white_space_collapse: WhiteSpaceCollapse::Collapse,
        ..Default::default()
    };
    let style = ParagraphStyle {
        root: root.clone(),
        first_line: Some(InlineStyle {
            text_transform: TextTransform::Uppercase,
            ..root.clone()
        }),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder
        .with_offset_mapping(true)
        .push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "ßa  ",
        )
        .push_text(TextSource::Generated { node: NodeId(8) }, "X")
        .push_text(TextSource::Generated { node: NodeId(9) }, "Y")
        .push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "ab",
        )
        .push_forced_break(NodeId(20))
        .push_text(TextSource::Generated { node: NodeId(8) }, "Z")
        .push_forced_break(NodeId(21))
        .push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 1,
            },
            "b",
        );
    let mut context = LayoutContext::new();
    let paragraph = builder.build(&mut context, &fonts.collection).unwrap();
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        1000.0,
        &AtomicSizes::EMPTY,
    );
    assert!(lines.len() >= 3);
    let mut repeated = lines.clone();
    repeated.push(lines[0].clone());
    repeated.reverse();
    repeated
}

fn make_layout(lines: &[shodo::Line]) -> AccessibleLayout<'_> {
    AccessibleLayout::new(lines)
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "time".into());
    #[cfg(not(feature = "allocation-counting"))]
    assert_ne!(
        mode, "alloc",
        "alloc mode requires --features allocation-counting"
    );
    assert!(matches!(mode.as_str(), "time" | "alloc"));

    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let mut rows = Vec::new();
    for count in [32, 128, 512, 1024] {
        let (lines, nodes) = generated_lines(count, &limits, &fonts.collection);
        #[cfg(feature = "allocation-counting")]
        let mut allocation = serde_json::Value::Null;
        #[cfg(not(feature = "allocation-counting"))]
        let allocation = serde_json::Value::Null;
        if mode == "alloc" {
            #[cfg(feature = "allocation-counting")]
            {
                let owned = lines.clone();
                let layout = make_layout(&owned);
                let position = layout.position(0, 0, Affinity::Downstream).unwrap();
                black_box(layout.to_source(position));
                let scope = ALLOC.begin().unwrap();
                let result = layout.from_source(SourcePosition {
                    origin: TextOrigin::Generated {
                        node: nodes[count / 2],
                    },
                    affinity: Affinity::Downstream,
                });
                black_box(&result);
                drop(result);
                allocation = json!(scope.finish());
            }
        }

        let query = SourcePosition {
            origin: TextOrigin::Generated {
                node: nodes[count / 2],
            },
            affinity: Affinity::Downstream,
        };
        let oracle_layout = make_layout(&lines);
        let expected = linear_generated(&oracle_layout, query);
        let actual = oracle_layout.from_source(query);
        assert_eq!(actual, expected, "single query differs for N={count}");
        let query_sha256 = digest(&[actual.clone()]);
        for affinity in [Affinity::Upstream, Affinity::Downstream] {
            for &node in &nodes {
                let source = SourcePosition {
                    origin: TextOrigin::Generated { node },
                    affinity,
                };
                assert_eq!(
                    oracle_layout.from_source(source),
                    linear_generated(&oracle_layout, source),
                    "{affinity:?} query differs for N={count}, {node:?}"
                );
            }
        }

        let mut linear_cold = Vec::with_capacity(SAMPLES);
        let mut indexed_cold = Vec::with_capacity(SAMPLES);
        let mut linear_warm = Vec::with_capacity(SAMPLES);
        let mut indexed_warm = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let (owned, _) = generated_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            let start = Instant::now();
            black_box(linear_generated(&layout, query));
            linear_cold.push(start.elapsed().as_nanos() as u64);

            let (owned, _) = generated_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            let start = Instant::now();
            black_box(layout.from_source(query));
            indexed_cold.push(start.elapsed().as_nanos() as u64);

            let (owned, _) = generated_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            black_box(linear_generated(&layout, query));
            let start = Instant::now();
            black_box(linear_generated(&layout, query));
            linear_warm.push(start.elapsed().as_nanos() as u64);

            let (owned, _) = generated_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            black_box(layout.from_source(query));
            let start = Instant::now();
            black_box(layout.from_source(query));
            indexed_warm.push(start.elapsed().as_nanos() as u64);
        }

        let sources = nodes
            .iter()
            .map(|&node| SourcePosition {
                origin: TextOrigin::Generated { node },
                affinity: Affinity::Downstream,
            })
            .collect::<Vec<_>>();
        let sweep_layout = make_layout(&lines);
        let expected = sources
            .iter()
            .map(|&source| linear_generated(&sweep_layout, source))
            .collect::<Vec<_>>();
        let actual = sources
            .iter()
            .map(|&source| sweep_layout.from_source(source))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "sweep differs for N={count}");
        let sweep_sha256 = digest(&actual);

        let mut linear_sweep = Vec::with_capacity(SAMPLES);
        let mut linear_warm_sweep = Vec::with_capacity(SAMPLES);
        let mut indexed_sweep = Vec::with_capacity(SAMPLES);
        let mut indexed_warm_sweep = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let (owned, _) = generated_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            let start = Instant::now();
            for &source in &sources {
                black_box(linear_generated(&layout, source));
            }
            linear_sweep.push(start.elapsed().as_nanos() as u64);

            let (owned, _) = generated_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            black_box(linear_generated(&layout, sources[0]));
            let start = Instant::now();
            for &source in &sources {
                black_box(linear_generated(&layout, source));
            }
            linear_warm_sweep.push(start.elapsed().as_nanos() as u64);

            let (owned, _) = generated_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            let start = Instant::now();
            for &source in &sources {
                black_box(layout.from_source(source));
            }
            indexed_sweep.push(start.elapsed().as_nanos() as u64);

            let (owned, _) = generated_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            black_box(layout.from_source(sources[0]));
            let start = Instant::now();
            for &source in &sources {
                black_box(layout.from_source(source));
            }
            indexed_warm_sweep.push(start.elapsed().as_nanos() as u64);
        }

        let (dom_oracle_lines, dom_nodes) = dom_lines(count, &limits, &fonts.collection);
        let dom_query = SourcePosition {
            origin: TextOrigin::Dom {
                node: dom_nodes[count / 2],
                offset: 0,
            },
            affinity: Affinity::Downstream,
        };
        let dom_sources = dom_nodes
            .iter()
            .map(|&node| SourcePosition {
                origin: TextOrigin::Dom { node, offset: 0 },
                affinity: Affinity::Downstream,
            })
            .collect::<Vec<_>>();
        let dom_oracle_layout = make_layout(&dom_oracle_lines);
        assert_eq!(
            dom_oracle_layout.from_source(dom_query),
            linear_generated(&dom_oracle_layout, dom_query),
            "DOM query differs for N={count}"
        );
        assert_eq!(
            dom_sources
                .iter()
                .map(|&source| dom_oracle_layout.from_source(source))
                .collect::<Vec<_>>(),
            dom_sources
                .iter()
                .map(|&source| linear_generated(&dom_oracle_layout, source))
                .collect::<Vec<_>>(),
            "DOM sweep differs for N={count}"
        );
        let mut dom_single_cold = Vec::with_capacity(SAMPLES);
        let mut dom_single_warm = Vec::with_capacity(SAMPLES);
        let mut dom_sweep_cold = Vec::with_capacity(SAMPLES);
        let mut dom_sweep_warm = Vec::with_capacity(SAMPLES);
        let mut dom_query_sha256 = String::new();
        let mut dom_sweep_sha256 = String::new();
        for _ in 0..SAMPLES {
            let (owned, _) = dom_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            let start = Instant::now();
            let result = layout.from_source(dom_query);
            dom_single_cold.push(start.elapsed().as_nanos() as u64);
            if dom_query_sha256.is_empty() {
                dom_query_sha256 = digest(&[result]);
            }

            let (owned, _) = dom_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            black_box(layout.from_source(dom_query));
            let start = Instant::now();
            black_box(layout.from_source(dom_query));
            dom_single_warm.push(start.elapsed().as_nanos() as u64);

            let (owned, _) = dom_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            let start = Instant::now();
            let results = dom_sources
                .iter()
                .map(|&source| layout.from_source(source))
                .collect::<Vec<_>>();
            dom_sweep_cold.push(start.elapsed().as_nanos() as u64);
            if dom_sweep_sha256.is_empty() {
                dom_sweep_sha256 = digest(&results);
            }

            let (owned, _) = dom_lines(count, &limits, &fonts.collection);
            let layout = make_layout(&owned);
            black_box(layout.from_source(dom_sources[0]));
            let start = Instant::now();
            for &source in &dom_sources {
                black_box(layout.from_source(source));
            }
            dom_sweep_warm.push(start.elapsed().as_nanos() as u64);
        }

        for samples in [
            &mut linear_cold,
            &mut indexed_cold,
            &mut linear_warm,
            &mut indexed_warm,
            &mut linear_sweep,
            &mut linear_warm_sweep,
            &mut indexed_sweep,
            &mut indexed_warm_sweep,
            &mut dom_single_cold,
            &mut dom_single_warm,
            &mut dom_sweep_cold,
            &mut dom_sweep_warm,
        ] {
            let _ = median(samples);
        }
        rows.push(json!({
            "count": count,
            "lines": lines.len(),
            "offset_mapping_struct_bytes": std::mem::size_of::<OffsetMapping>(),
            "generated_index_cache_slot_bytes": std::mem::size_of::<
                std::sync::OnceLock<Box<[u8]>>,
            >(),
            "result_count": actual[count / 2].len(),
            "single_query_sha256": query_sha256,
            "sweep_sha256": sweep_sha256,
            "allocation_after_mapping_warm": allocation,
            "linear_single_cold_p50_ns": median(&mut linear_cold),
            "indexed_single_cold_p50_ns": median(&mut indexed_cold),
            "linear_single_warm_p50_ns": median(&mut linear_warm),
            "indexed_single_warm_p50_ns": median(&mut indexed_warm),
            "linear_sweep_cold_p50_ns": median(&mut linear_sweep),
            "linear_sweep_warm_p50_ns": median(&mut linear_warm_sweep),
            "indexed_sweep_cold_p50_ns": median(&mut indexed_sweep),
            "indexed_sweep_warm_p50_ns": median(&mut indexed_warm_sweep),
            "dom_single_query_sha256": dom_query_sha256,
            "dom_sweep_sha256": dom_sweep_sha256,
            "dom_single_cold_p50_ns": median(&mut dom_single_cold),
            "dom_single_warm_p50_ns": median(&mut dom_single_warm),
            "dom_sweep_cold_p50_ns": median(&mut dom_sweep_cold),
            "dom_sweep_warm_p50_ns": median(&mut dom_sweep_warm),
        }));
    }
    let edge_lines = edge_lines();
    let edge_layout = make_layout(&edge_lines);
    let mut edge_results = Vec::new();
    for affinity in [Affinity::Upstream, Affinity::Downstream] {
        for node in [NodeId(8), NodeId(9), NodeId(99)] {
            let source = SourcePosition {
                origin: TextOrigin::Generated { node },
                affinity,
            };
            let actual = edge_layout.from_source(source);
            assert_eq!(
                actual,
                linear_generated(&edge_layout, source),
                "edge query differs for {affinity:?}, {node:?}"
            );
            edge_results.push(actual);
        }
    }
    rows.push(json!({
        "kind": "affinity-boundary-multiline-repeated",
        "lines": edge_lines.len(),
        "exact_queries": edge_results.len(),
        "positions_sha256": digest(&edge_results),
    }));
    println!("{}", serde_json::to_string(&rows).unwrap());
}
