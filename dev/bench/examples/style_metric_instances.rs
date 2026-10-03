//! Paired build timing and allocation probe for StyleMetrics font instances.
//! Run without features for timing, then with `allocation-counting` for allocs.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::geometry::LogicalRect;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{
    FontFamily, FontMetricKind, FontSizeAdjust, FontVariation, InlineStyle, LineOptions,
    ParagraphStyle,
};
use shodo::{AtomicSizes, Fragment, LayoutContext, Paragraph, ParagraphBuilder};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const WARMUP: usize = 3;
const STYLE_COUNT: usize = 64;

#[derive(Clone, Copy)]
enum FontCase {
    Latin,
    CjkFallback,
    Variable,
    SizeAdjust,
}

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    font: FontCase,
    text: &'static str,
}

const CASES: [Case; 4] = [
    Case {
        name: "latin-multi-style",
        font: FontCase::Latin,
        text: "a",
    },
    Case {
        name: "cjk-fallback-multi-style",
        font: FontCase::CjkFallback,
        text: "水",
    },
    Case {
        name: "variable-multi-style",
        font: FontCase::Variable,
        text: "😀",
    },
    Case {
        name: "size-adjust-multi-style",
        font: FontCase::SizeAdjust,
        text: "a",
    },
];

fn family(case: FontCase) -> Vec<FontFamily> {
    match case {
        FontCase::Latin | FontCase::SizeAdjust => {
            vec![FontFamily::Named("Shodo Fixture Latin".into())]
        }
        FontCase::CjkFallback => vec![
            FontFamily::Named("Shodo Fixture Latin".into()),
            FontFamily::Named("Shodo Fixture CJK".into()),
        ],
        FontCase::Variable => vec![FontFamily::Named("Shodo Fixture Emoji".into())],
    }
}

fn style(case: FontCase, index: Option<usize>) -> InlineStyle {
    let mut style = InlineStyle {
        font_families: family(case),
        font_size: index.map_or(16.0, |index| 16.125 + index as f32 * 0.015),
        ..Default::default()
    };
    match case {
        FontCase::Variable => {
            style.font_weight = 700.0;
            style.font_variations = vec![FontVariation {
                tag: *b"wght",
                value: 700.0,
            }];
        }
        FontCase::SizeAdjust => {
            style.font_size_adjust = Some(FontSizeAdjust {
                metric: FontMetricKind::ExHeight,
                value: 0.5,
            });
        }
        FontCase::Latin | FontCase::CjkFallback => {}
    }
    style
}

fn builder(case: Case, limits: &Limits) -> ParagraphBuilder {
    let mut root = style(case.font, None);
    root.paint.color = [255, 255, 255, 255];
    let paragraph_style = ParagraphStyle {
        root: root.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, limits);
    for index in 0..STYLE_COUNT {
        let mut style = style(case.font, Some(index));
        style.paint.color = [index as u8, (index >> 8) as u8, 17, 255];
        let node = NodeId(index as u64 + 1);
        builder
            .open_inline(node, &style, InlineEdges::default())
            .push_text(TextSource::Generated { node }, case.text)
            .close_inline();
    }
    builder
}

fn add_usize(hash: &mut Sha256, value: usize) {
    hash.update((value as u64).to_le_bytes());
}

fn add_f32(hash: &mut Sha256, value: f32) {
    hash.update(value.to_bits().to_le_bytes());
}

fn add_rect(hash: &mut Sha256, rect: LogicalRect) {
    for value in [
        rect.inline_start,
        rect.block_start,
        rect.inline_size,
        rect.block_size,
    ] {
        add_f32(hash, value);
    }
}

fn add_debug(hash: &mut Sha256, value: impl std::fmt::Debug) {
    let value = format!("{value:?}");
    add_usize(hash, value.len());
    hash.update(value.as_bytes());
}

struct Snapshot {
    geometry_sha256: String,
    instance_sha256: String,
    warnings: Vec<String>,
}

fn snapshot(
    paragraph: &Paragraph,
    context: &mut LayoutContext,
    case: Case,
    fonts: &shodo_fixtures::EmojiFixtureFonts,
) -> Snapshot {
    let lines = paragraph.break_all(
        context,
        &LineOptions::default(),
        1_000_000.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1, "fixture should remain a single line");
    let spans: Vec<_> = lines.iter().flat_map(|line| line.paint_spans()).collect();
    assert_eq!(
        spans.len(),
        STYLE_COUNT,
        "paint span count for {}",
        case.name
    );
    let colors: std::collections::BTreeSet<_> = spans.iter().map(|span| span.style.color).collect();
    assert_eq!(colors.len(), STYLE_COUNT, "paint colors for {}", case.name);

    let mut geometry = Sha256::new();
    let mut instances = Sha256::new();
    add_usize(&mut geometry, lines.len());
    for line in &lines {
        add_usize(&mut geometry, line.text_range().start);
        add_usize(&mut geometry, line.text_range().end);
        for value in [
            line.inline_size(),
            line.block_size(),
            line.block_offset(),
            line.hang_start(),
            line.hang_end(),
        ] {
            add_f32(&mut geometry, value);
        }
        for span in line.paint_spans() {
            add_usize(&mut geometry, span.text_range.start);
            add_usize(&mut geometry, span.text_range.end);
            geometry.update(span.style.color);
            add_rect(&mut geometry, span.rect);
        }
        for fragment in line.fragments() {
            match fragment {
                Fragment::GlyphRun(run) => {
                    add_usize(&mut instances, run.font().layer() as usize);
                    add_usize(&mut instances, run.font().index() as usize);
                    add_usize(&mut instances, run.style_index() as usize);
                    add_usize(&mut instances, run.text_range().start);
                    add_usize(&mut instances, run.text_range().end);
                    add_f32(&mut instances, run.font_size());
                    add_debug(&mut instances, run.metrics());
                    add_debug(&mut instances, run.vertical_metrics());
                    add_debug(&mut instances, run.variations());
                    add_debug(&mut instances, run.normalized_coords());
                    add_debug(&mut instances, run.script());
                    add_debug(&mut instances, run.source());
                    for (index, glyph) in run.glyphs().enumerate() {
                        add_usize(&mut instances, glyph.id as usize);
                        add_usize(&mut instances, glyph.cluster as usize);
                        add_f32(&mut instances, glyph.inline_position);
                        add_f32(&mut instances, glyph.block_offset);
                        add_f32(&mut instances, glyph.advance);
                        if let Some((x, y)) = run.glyph_origin(index) {
                            add_f32(&mut instances, x);
                            add_f32(&mut instances, y);
                        }
                    }
                }
                other => add_debug(&mut instances, other),
            }
        }
    }

    if matches!(case.font, FontCase::Variable) {
        assert!(
            lines
                .iter()
                .flat_map(|line| line.fragments())
                .any(|fragment| {
                    matches!(fragment, Fragment::GlyphRun(run) if run.font() == fonts.emoji_ids[1])
                }),
            "variable emoji face should be selected"
        );
    }
    if matches!(case.font, FontCase::CjkFallback) {
        assert!(
            lines
                .iter()
                .flat_map(|line| line.fragments())
                .any(|fragment| {
                    matches!(fragment, Fragment::GlyphRun(run) if run.font() == fonts.base.ids[1])
                }),
            "CJK fallback face should shape the CJK input"
        );
    }

    let mut warnings: Vec<_> = paragraph
        .warnings()
        .iter()
        .map(|warning| format!("{:?}:{}", warning.kind, warning.message))
        .collect();
    warnings.extend(
        context
            .take_warnings()
            .into_iter()
            .map(|warning| format!("{:?}:{}", warning.kind, warning.message)),
    );

    Snapshot {
        geometry_sha256: format!("{:x}", geometry.finalize()),
        instance_sha256: format!("{:x}", instances.finalize()),
        warnings,
    }
}

fn sample(
    case: Case,
    mode: &str,
    fonts: &shodo_fixtures::EmojiFixtureFonts,
    index: usize,
) -> Value {
    let limits = Limits::default();
    let builder = builder(case, &limits);
    let mut context = LayoutContext::new();
    let (paragraph, measurement) = match mode {
        "time" => {
            let start = Instant::now();
            let paragraph = black_box(builder)
                .build(&mut context, &fonts.base.collection)
                .unwrap();
            let elapsed = start.elapsed().as_nanos() as u64;
            black_box(&paragraph);
            (paragraph, json!({"ns": elapsed}))
        }
        "alloc" => {
            #[cfg(feature = "allocation-counting")]
            {
                let scope = ALLOC.begin().unwrap();
                let paragraph = black_box(builder)
                    .build(&mut context, &fonts.base.collection)
                    .unwrap();
                let allocation = scope.finish();
                black_box(&paragraph);
                (paragraph, json!({"allocation": allocation}))
            }
            #[cfg(not(feature = "allocation-counting"))]
            panic!("alloc mode requires --features allocation-counting");
        }
        _ => panic!("mode must be time or alloc"),
    };
    let output = snapshot(&paragraph, &mut context, case, fonts);
    assert!(
        output.warnings.is_empty(),
        "unexpected warnings: {:?}",
        output.warnings
    );
    json!({
        "schema": 1,
        "mode": mode,
        "fixture": case.name,
        "style_count": STYLE_COUNT,
        "sample": index,
        "measurement": measurement,
        "output": {
            "geometry_sha256": output.geometry_sha256,
            "instance_sha256": output.instance_sha256,
            "warnings": output.warnings,
        }
    })
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "time".into());
    let expected_mode = if cfg!(feature = "allocation-counting") {
        "alloc"
    } else {
        "time"
    };
    assert_eq!(
        mode, expected_mode,
        "use separate time and allocation builds"
    );
    let samples = if mode == "time" { 21 } else { 9 };
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_emoji_fonts(&limits).unwrap();
    for case in CASES {
        for _ in 0..WARMUP {
            black_box(sample(case, &mode, &fonts, usize::MAX));
        }
        for index in 0..samples {
            println!(
                "{}",
                serde_json::to_string(&sample(case, &mode, &fonts, index)).unwrap()
            );
        }
    }
}
